use std::{
    collections::VecDeque,
    sync::atomic::{AtomicUsize, Ordering},
};

use super::*;

#[derive(Default)]
struct FakeState {
    hints: Vec<String>,
    actions: Vec<&'static str>,
    audio: Vec<f32>,
    results: VecDeque<String>,
}

struct FakeDecoder {
    state: Arc<Mutex<FakeState>>,
    dropped: Arc<AtomicUsize>,
}

impl ZipformerDecoder for FakeDecoder {
    fn create_stream(&self, hotwords: &str) -> Box<dyn ZipformerStream> {
        if let Ok(mut state) = self.state.lock() {
            state.hints.push(hotwords.to_owned());
        }
        Box::new(FakeStream {
            state: Arc::clone(&self.state),
            dropped: Arc::clone(&self.dropped),
        })
    }
}

struct FakeStream {
    state: Arc<Mutex<FakeState>>,
    dropped: Arc<AtomicUsize>,
}

impl ZipformerStream for FakeStream {
    fn accept_audio(&mut self, samples: &[f32]) {
        if let Ok(mut state) = self.state.lock() {
            state.actions.push("audio");
            state.audio.extend_from_slice(samples);
        }
    }

    fn decode_ready(&mut self) {
        if let Ok(mut state) = self.state.lock() {
            state.actions.push("decode");
        }
    }

    fn text(&mut self) -> Option<String> {
        self.state.lock().ok()?.results.pop_front()
    }

    fn finish_input(&mut self) {
        if let Ok(mut state) = self.state.lock() {
            state.actions.push("finish");
        }
    }
}

impl Drop for FakeStream {
    fn drop(&mut self) {
        self.dropped.fetch_add(1, Ordering::Relaxed);
    }
}

fn recognizer(
    state: &Arc<Mutex<FakeState>>,
    dropped: &Arc<AtomicUsize>,
) -> ZipformerSpeechRecognizer {
    ZipformerSpeechRecognizer {
        decoder: Arc::new(FakeDecoder {
            state: Arc::clone(state),
            dropped: Arc::clone(dropped),
        }),
        vocabulary: "ABCDEFGHIJKLMNOPQRSTUVWXYZ 声墨".chars().collect(),
    }
}

#[test]
fn sessions_snapshot_hints_without_leaking_into_the_next_session()
-> Result<(), SpeechRecognitionError> {
    let state = Arc::new(Mutex::new(FakeState::default()));
    let recognizer = recognizer(&state, &Arc::new(AtomicUsize::new(0)));
    let first = recognizer.start(
        SpeechRecognitionHints::from_terms(vec!["声墨".to_owned(), "Saymore".to_owned()]),
        Arc::new(|_| {}),
    )?;
    let second = recognizer.start(
        SpeechRecognitionHints::from_terms(vec!["Slint".to_owned()]),
        Arc::new(|_| {}),
    )?;
    let third = recognizer.start(SpeechRecognitionHints::default(), Arc::new(|_| {}))?;
    second.cancel();
    first.cancel();
    third.cancel();
    assert_eq!(
        vec!["声墨/SAYMORE", "SLINT", ""],
        state.lock().map_err(lock_error)?.hints
    );
    Ok(())
}

#[test]
fn forwards_ordered_pcm_emits_changed_partials_and_flushes_at_finish()
-> Result<(), SpeechRecognitionError> {
    let state = Arc::new(Mutex::new(FakeState {
        results: [" partial ", "partial", " final text "]
            .map(str::to_owned)
            .into(),
        ..Default::default()
    }));
    let recognizer = recognizer(&state, &Arc::new(AtomicUsize::new(0)));
    let partials = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&partials);
    let session = recognizer.start(
        SpeechRecognitionHints::default(),
        Arc::new(move |text| {
            if let Ok(mut partials) = captured.lock() {
                partials.push(text);
            }
        }),
    )?;
    session.push_audio(Vec::new())?;
    session.push_audio(vec![i16::MIN, 0, i16::MAX])?;
    session.push_audio(vec![1; 3_201])?;
    assert_eq!("final text", session.finish()?);
    assert_eq!(vec!["partial"], *partials.lock().map_err(lock_error)?);
    let state = state.lock().map_err(lock_error)?;
    let mut expected = vec![-1.0, 0.0, f32::from(i16::MAX) / 32_768.0];
    expected.extend(vec![1.0 / 32_768.0; 3_201]);
    expected.extend(vec![0.0; TAIL_PADDING_SAMPLES]);
    assert_eq!(expected, state.audio);
    assert_eq!(
        vec![
            "audio", "decode", "audio", "decode", "audio", "decode", "audio", "decode", "audio",
            "finish", "decode"
        ],
        state.actions
    );
    Ok(())
}

#[test]
fn cancel_and_empty_input_drop_streams_without_final_decoding() -> Result<(), SpeechRecognitionError>
{
    let state = Arc::new(Mutex::new(FakeState::default()));
    let dropped = Arc::new(AtomicUsize::new(0));
    let recognizer = recognizer(&state, &dropped);
    let empty = recognizer.start(SpeechRecognitionHints::default(), Arc::new(|_| {}))?;
    empty.push_audio(Vec::new())?;
    assert!(matches!(
        empty.finish(),
        Err(SpeechRecognitionError::Protocol(_))
    ));
    let cancelled = recognizer.start(SpeechRecognitionHints::default(), Arc::new(|_| {}))?;
    cancelled.push_audio(vec![1; 100])?;
    cancelled.cancel();
    assert_eq!(2, dropped.load(Ordering::Relaxed));
    assert_eq!(
        vec!["audio", "decode"],
        state.lock().map_err(lock_error)?.actions
    );
    Ok(())
}

#[test]
fn rejects_an_empty_native_final_result() -> Result<(), SpeechRecognitionError> {
    let state = Arc::new(Mutex::new(FakeState::default()));
    let recognizer = recognizer(&state, &Arc::new(AtomicUsize::new(0)));
    let session = recognizer.start(SpeechRecognitionHints::default(), Arc::new(|_| {}))?;
    session.push_audio(vec![0; 1_600])?;
    assert!(matches!(
        session.finish(),
        Err(SpeechRecognitionError::Protocol(_))
    ));
    Ok(())
}

#[test]
fn incomplete_or_wrong_sized_artifacts_are_rejected_before_native_loading()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    assert!(matches!(
        ZipformerSpeechRecognizer::load(directory.path()),
        Err(SpeechRecognitionError::NotConfigured)
    ));
    for (name, bytes) in MODEL_FILES {
        fs::File::create(directory.path().join(name))?.set_len(bytes)?;
    }
    assert!(validate_files(directory.path()).is_ok());
    fs::File::create(directory.path().join("tokens.txt"))?.set_len(1)?;
    assert!(matches!(
        ZipformerSpeechRecognizer::load(directory.path()),
        Err(SpeechRecognitionError::Protocol(_))
    ));
    Ok(())
}
