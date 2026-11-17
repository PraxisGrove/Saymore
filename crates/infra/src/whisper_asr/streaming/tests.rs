#![allow(clippy::panic_in_result_fn)]
use super::*;
use crate::whisper_asr::MAX_SEGMENT_SAMPLES;
use std::{error::Error, time::Duration};

type Calls = Arc<Mutex<Vec<(Vec<i16>, String)>>>;
struct Decoder {
    calls: Calls,
}
impl WhisperDecoder for Decoder {
    fn decode(&self, audio: &[i16], prompt: &str) -> Result<String, SpeechRecognitionError> {
        self.calls
            .lock()
            .map_err(|_| protocol("test lock"))?
            .push((audio.to_vec(), prompt.into()));
        Ok(if audio.len() == CHUNK_SAMPLES {
            "draft"
        } else {
            "final"
        }
        .into())
    }
    fn preview(&self, audio: &[i16], prompt: &str) -> Result<String, SpeechRecognitionError> {
        self.decode(audio, prompt)
            .map(|text| format!("{text} preview"))
    }
}
fn fixture() -> (Arc<dyn WhisperDecoder>, Calls) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    (
        Arc::new(Decoder {
            calls: calls.clone(),
        }),
        calls,
    )
}

#[test]
fn preview_is_replaced_and_finish_confirms_with_full_context() -> Result<(), Box<dyn Error>> {
    let (decoder, calls) = fixture();
    let (tx, rx) = mpsc::channel();
    let session = start(
        decoder,
        "Saymore".into(),
        Arc::new(move |s| {
            let _ = tx.send(s);
        }),
    )?;
    session.push_audio(vec![1; CHUNK_SAMPLES])?;
    assert_eq!(rx.recv_timeout(Duration::from_secs(5))?, "draft preview");
    session.push_audio(vec![2; CHUNK_SAMPLES])?;
    assert_eq!(rx.recv_timeout(Duration::from_secs(5))?, "final preview");
    assert_eq!(session.finish()?, "final");
    let calls = calls.lock().map_err(|_| "test lock")?;
    assert_eq!(calls.len(), 3);
    assert!(calls.iter().all(|(_, prompt)| prompt == "Saymore"));
    assert_eq!(
        calls[1].0,
        [vec![1; CHUNK_SAMPLES], vec![2; CHUNK_SAMPLES]].concat()
    );
    Ok(())
}

#[test]
fn window_rollover_preserves_audio_repeated_text_and_short_tail() -> Result<(), Box<dyn Error>> {
    let (decoder, calls) = fixture();
    let session = start(decoder, String::new(), Arc::new(|_| {}))?;
    let mut audio = vec![3; 2 * MAX_SEGMENT_SAMPLES];
    audio.extend(vec![4; 17]);
    session.push_audio(audio.clone())?;
    assert_eq!(session.finish()?, "final final final");
    let calls = calls.lock().map_err(|_| "test lock")?;
    let finalized: Vec<i16> = calls
        .iter()
        .filter(|(s, _)| s.len() == MAX_SEGMENT_SAMPLES || s.len() == 17)
        .flat_map(|(s, _)| s.iter().copied())
        .collect();
    assert_eq!(finalized, audio);
    assert!(
        calls
            .iter()
            .all(|(s, p)| s.len() <= MAX_SEGMENT_SAMPLES && p.is_empty())
    );
    Ok(())
}

#[test]
fn empty_cancelled_and_oversized_sessions_do_not_decode_buffered_audio()
-> Result<(), Box<dyn Error>> {
    let (decoder, calls) = fixture();
    assert!(
        start(decoder.clone(), String::new(), Arc::new(|_| {}))?
            .finish()
            .is_err()
    );
    let session = start(decoder.clone(), String::new(), Arc::new(|_| {}))?;
    session.push_audio(vec![1; 17])?;
    session.cancel();
    let session = start(decoder, String::new(), Arc::new(|_| {}))?;
    assert!(session.push_audio(vec![0; MAX_SAMPLES + 1]).is_err());
    assert!(session.finish().is_err());
    assert!(calls.lock().map_err(|_| "test lock")?.is_empty());
    Ok(())
}

struct RevisingDecoder {
    calls: std::sync::atomic::AtomicUsize,
}
impl WhisperDecoder for RevisingDecoder {
    fn decode(&self, _: &[i16], _: &str) -> Result<String, SpeechRecognitionError> {
        Ok("confirmed".into())
    }
    fn preview(&self, _: &[i16], _: &str) -> Result<String, SpeechRecognitionError> {
        match self.calls.fetch_add(1, Ordering::Relaxed) {
            0 => Ok("incorrect draft".into()),
            1 => Ok(String::new()),
            _ => Err(protocol("decoder failed")),
        }
    }
}

#[test]
fn empty_revisions_clear_previews_and_decode_failures_are_not_finalized()
-> Result<(), Box<dyn Error>> {
    let (tx, rx) = mpsc::channel();
    let decoder = Arc::new(RevisingDecoder { calls: 0.into() });
    let session = start(
        decoder,
        String::new(),
        Arc::new(move |s| {
            let _ = tx.send(s);
        }),
    )?;
    session.push_audio(vec![1; CHUNK_SAMPLES])?;
    assert_eq!(rx.recv_timeout(Duration::from_secs(5))?, "incorrect draft");
    session.push_audio(vec![1; CHUNK_SAMPLES])?;
    assert_eq!(rx.recv_timeout(Duration::from_secs(5))?, "");
    session.push_audio(vec![1; CHUNK_SAMPLES])?;
    // The decoder error closes the callback channel, even before finish.
    assert!(matches!(
        rx.recv_timeout(Duration::from_secs(5)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    ));
    assert_eq!(session.finish(), Err(protocol("decoder failed")));
    Ok(())
}

struct BlockedDecoder {
    started: mpsc::Sender<usize>,
    release: Mutex<mpsc::Receiver<()>>,
}
impl WhisperDecoder for BlockedDecoder {
    fn decode(&self, audio: &[i16], _: &str) -> Result<String, SpeechRecognitionError> {
        self.started
            .send(audio.len())
            .map_err(|_| protocol("test channel"))?;
        self.release
            .lock()
            .map_err(|_| protocol("test lock"))?
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| protocol("test release"))?;
        Ok(format!("text{}", audio.len()))
    }
}

#[test]
fn slow_decoder_coalesces_previews_and_cancellation_suppresses_late_results()
-> Result<(), Box<dyn Error>> {
    let (started, calls) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let decoder = Arc::new(BlockedDecoder {
        started,
        release: Mutex::new(blocked),
    });
    let (tx, rx) = mpsc::channel();
    let session = start(
        decoder,
        String::new(),
        Arc::new(move |s| {
            let _ = tx.send(s);
        }),
    )?;
    session.push_audio(vec![1; CHUNK_SAMPLES])?;
    assert_eq!(calls.recv_timeout(Duration::from_secs(5))?, CHUNK_SAMPLES);
    session.push_audio(vec![2; CHUNK_SAMPLES * 5])?;
    release.send(())?;
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(5))?,
        format!("text{CHUNK_SAMPLES}")
    );
    assert_eq!(
        calls.recv_timeout(Duration::from_secs(5))?,
        CHUNK_SAMPLES * 6
    );
    session.cancel();
    release.send(())?;
    // Wait for the worker to drop its callback; no cancelled result may escape.
    assert!(matches!(
        rx.recv_timeout(Duration::from_secs(5)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    ));
    Ok(())
}
