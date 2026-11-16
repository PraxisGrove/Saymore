#![allow(clippy::panic_in_result_fn)]
use super::super::StreamingText;
use super::*;
use std::time::Duration;

type Seen = Arc<Mutex<Vec<(usize, i16, String, String)>>>;
struct Decoder {
    seen: Seen,
}
impl Qwen3Decoder for Decoder {
    fn decode(&self, _: &[i16], _: &str) -> Result<String, SpeechRecognitionError> {
        Err(protocol("unexpected segment decode"))
    }
    fn decode_stream(
        &self,
        samples: &[i16],
        hints: &str,
        prefix: &str,
    ) -> Result<StreamingText, SpeechRecognitionError> {
        let mut seen = self.seen.lock().map_err(|_| protocol("test lock"))?;
        seen.push((
            samples.len(),
            samples.last().copied().unwrap_or_default(),
            hints.into(),
            prefix.into(),
        ));
        let index = seen.len();
        Ok(StreamingText {
            text: format!("hypothesis {index}"),
            raw: format!("raw {index}"),
        })
    }
}

#[test]
fn cumulative_audio_uses_two_unfixed_chunks_and_flushes_tail_with_replaceable_results()
-> Result<(), SpeechRecognitionError> {
    let seen: Seen = Arc::default();
    let (updates, received) = mpsc::channel();
    let session = start(
        Arc::new(Decoder { seen: seen.clone() }),
        "Saymore".into(),
        Arc::new(move |text| {
            let _ = updates.send(text);
        }),
    )?;
    let mut hypotheses = Vec::new();
    for _ in 0..3 {
        session.push_audio(vec![1; CHUNK_SAMPLES])?;
        hypotheses.push(
            received
                .recv_timeout(Duration::from_secs(2))
                .map_err(|_| protocol("missing test preview"))?,
        );
    }
    session.push_audio(vec![7; 16_000])?;
    assert_eq!("hypothesis 4", session.finish()?);
    hypotheses.extend(received.iter());
    assert_eq!(
        vec![
            (32_000, 1, "Saymore".into(), "".into()),
            (64_000, 1, "Saymore".into(), "".into()),
            (96_000, 1, "Saymore".into(), "raw 2".into()),
            (112_000, 7, "Saymore".into(), "raw 3".into()),
        ],
        *seen.lock().map_err(|_| protocol("test lock"))?
    );
    assert_eq!(
        vec![
            "hypothesis 1",
            "hypothesis 2",
            "hypothesis 3",
            "hypothesis 4"
        ],
        hypotheses
    );
    Ok(())
}

#[test]
fn exact_chunk_boundary_is_not_decoded_again_and_empty_input_is_rejected()
-> Result<(), SpeechRecognitionError> {
    let seen: Seen = Arc::default();
    let decoder = Arc::new(Decoder { seen: seen.clone() });
    let empty = start(decoder.clone(), String::new(), Arc::new(|_| {}))?;
    assert!(empty.finish().is_err());
    let (send, ready) = mpsc::channel();
    let session = start(
        decoder,
        String::new(),
        Arc::new(move |_| {
            let _ = send.send(());
        }),
    )?;
    for _ in 0..2 {
        session.push_audio(vec![1; CHUNK_SAMPLES])?;
        ready
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| protocol("missing test preview"))?;
    }
    assert_eq!("hypothesis 2", session.finish()?);
    assert_eq!(2, seen.lock().map_err(|_| protocol("test lock"))?.len());
    Ok(())
}

#[test]
fn oversized_input_fails_before_queueing_or_decoding() -> Result<(), SpeechRecognitionError> {
    let seen: Seen = Arc::default();
    let session = start(
        Arc::new(Decoder { seen: seen.clone() }),
        String::new(),
        Arc::new(|_| {}),
    )?;
    assert!(session.push_audio(vec![0; MAX_SAMPLES + 1]).is_err());
    assert!(session.finish().is_err());
    assert!(seen.lock().map_err(|_| protocol("test lock"))?.is_empty());
    Ok(())
}

struct Blocked {
    entered: mpsc::Sender<usize>,
    release: Mutex<mpsc::Receiver<()>>,
}
impl Qwen3Decoder for Blocked {
    fn decode(&self, _: &[i16], _: &str) -> Result<String, SpeechRecognitionError> {
        Err(protocol("unexpected segment decode"))
    }
    fn decode_stream(
        &self,
        samples: &[i16],
        _: &str,
        _: &str,
    ) -> Result<StreamingText, SpeechRecognitionError> {
        let _ = self.entered.send(samples.len());
        self.release
            .lock()
            .map_err(|_| protocol("test lock"))?
            .recv()
            .map_err(|_| protocol("test release"))?;
        Ok(StreamingText {
            text: "late result".into(),
            raw: "late result".into(),
        })
    }
}

#[test]
fn decoding_starts_before_finish_and_cancel_discards_late_results()
-> Result<(), Box<dyn std::error::Error>> {
    let (entered, began) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let (updates, observed) = mpsc::channel();
    let session = start(
        Arc::new(Blocked {
            entered,
            release: Mutex::new(wait),
        }),
        String::new(),
        Arc::new(move |text| {
            let _ = updates.send(text);
        }),
    )?;
    session.push_audio(vec![1; CHUNK_SAMPLES])?;
    began.recv_timeout(Duration::from_secs(2))?;
    session.push_audio(vec![2; CHUNK_SAMPLES])?;
    session.cancel();
    release.send(())?;
    assert!(matches!(
        observed.recv_timeout(Duration::from_secs(2)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    ));
    Ok(())
}

#[test]
fn slow_inference_coalesces_pending_steps_without_losing_audio()
-> Result<(), Box<dyn std::error::Error>> {
    let (entered, began) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let (updates, observed) = mpsc::channel();
    let session = start(
        Arc::new(Blocked {
            entered,
            release: Mutex::new(wait),
        }),
        String::new(),
        Arc::new(move |text| {
            let _ = updates.send(text);
        }),
    )?;
    session.push_audio(vec![1; CHUNK_SAMPLES])?;
    assert_eq!(CHUNK_SAMPLES, began.recv_timeout(Duration::from_secs(2))?);
    session.push_audio(vec![2; 5 * CHUNK_SAMPLES])?;
    release.send(())?;
    assert_eq!(
        6 * CHUNK_SAMPLES,
        began.recv_timeout(Duration::from_secs(2))?
    );
    release.send(())?;
    assert_eq!("late result", session.finish()?);
    assert_eq!(vec!["late result"], observed.iter().collect::<Vec<_>>());
    Ok(())
}
