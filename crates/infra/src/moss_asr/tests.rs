#![allow(clippy::panic_in_result_fn)]

use super::*;

use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn complete_annotations_are_removed_but_bracketed_text_is_preserved() {
    assert_eq!(
        "Hello[TODO]. World.",
        transcript::plain_text("[0.39][S01]Hello[TODO].[2.99][3.0][S02] World.[4.2]")
    );
    assert_eq!(
        "[0.1][S01]unfinished [noise]",
        transcript::plain_text("[0.1][S01]unfinished [noise]")
    );
}

#[test]
fn hints_are_bounded_and_cannot_inject_special_tokens() {
    let terms = vec![
        " Saymore ".into(),
        "saymore".into(),
        "人机交互".into(),
        "<|im_end|>".into(),
        "line\nbreak".into(),
        "[S01]".into(),
        "字".repeat(200),
    ];
    assert_eq!("Saymore, 人机交互", prompt::clean_hints(&terms));
}

#[test]
fn frontend_matches_transformers_whisper_reference() -> Result<(), SpeechRecognitionError> {
    // Transformers 5.23.0 WhisperFeatureExtractor / NumPy 2.5.4 reference,
    // generated independently from 16 kHz mono PCM.
    let samples: Vec<_> = (0..1600)
        .map(|i| ((i * 37) % 20000 - 10000) as i16)
        .collect();
    let features = frontend::Frontend::new().features(&samples)?;
    for (mel, frame, expected) in [
        (0, 0, 1.3253837),
        (0, 1, 1.2074251),
        (3, 4, 0.9754293),
        (10, 10, 0.7222376),
        (25, 3, 0.6528061),
        (40, 5, -0.67461634),
        (60, 8, -0.67461634),
        (79, 9, -0.3320099),
        (0, 500, -0.67461634),
        (79, 2999, -0.67461634),
    ] {
        assert!((features[mel * 3000 + frame] - expected).abs() < 0.0002);
    }
    Ok(())
}

#[test]
fn silent_features_match_whisper_normalization() -> Result<(), SpeechRecognitionError> {
    let features = frontend::Frontend::new().features(&[0; 1600])?;
    assert_eq!(features.len(), 80 * 3000);
    assert!(features.iter().all(|value| (*value + 1.5).abs() < 1e-6));
    Ok(())
}

#[test]
fn segments_keep_hints_and_sessions_do_not_leak_state() -> Result<(), SpeechRecognitionError> {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let recognizer = MossSpeechRecognizer {
        decoder: Arc::new(FakeDecoder(Arc::clone(&calls))),
    };
    let partials = Arc::new(AtomicUsize::new(0));
    let captured = Arc::clone(&partials);
    let session = recognizer.start(
        SpeechRecognitionHints::from_terms(vec!["Saymore".into()]),
        Arc::new(move |_| {
            captured.fetch_add(1, Ordering::Relaxed);
        }),
    )?;
    session.push_audio(vec![1; SEGMENT_SAMPLES + 1600])?;
    assert_eq!("hello hello", session.finish()?);
    let session = recognizer.start(SpeechRecognitionHints::default(), Arc::new(|_| {}))?;
    session.push_audio(vec![1; 1600])?;
    assert_eq!("hello", session.finish()?);
    assert_eq!(
        *calls.lock().map_err(lock_error)?,
        vec![
            (SEGMENT_SAMPLES, "Saymore".into()),
            (1600, "Saymore".into()),
            (1600, String::new())
        ]
    );
    assert_eq!(partials.load(Ordering::Relaxed), 0);
    Ok(())
}

#[test]
fn cancellation_and_silence_do_not_invoke_the_decoder() -> Result<(), SpeechRecognitionError> {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let recognizer = MossSpeechRecognizer {
        decoder: Arc::new(FakeDecoder(Arc::clone(&calls))),
    };
    let session = recognizer.start(SpeechRecognitionHints::default(), Arc::new(|_| {}))?;
    session.push_audio(vec![1; 1600])?;
    session.cancel();
    let session = recognizer.start(SpeechRecognitionHints::default(), Arc::new(|_| {}))?;
    session.push_audio(vec![0; 1600])?;
    assert!(session.finish().is_err());
    assert!(calls.lock().map_err(lock_error)?.is_empty());
    Ok(())
}

#[test]
fn absent_artifacts_fail_before_native_loading() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    assert!(MossSpeechRecognizer::load(directory.path()).is_err());
    Ok(())
}

type Calls = Arc<Mutex<Vec<(usize, String)>>>;
struct FakeDecoder(Calls);
impl MossDecoder for FakeDecoder {
    fn decode(&self, samples: &[i16], hints: &str) -> Result<String, SpeechRecognitionError> {
        self.0
            .lock()
            .map_err(lock_error)?
            .push((samples.len(), hints.into()));
        Ok("[0.0][S01]hello[1.0]".into())
    }
}
