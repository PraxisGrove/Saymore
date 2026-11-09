#![allow(clippy::panic_in_result_fn)]

use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

#[test]
fn long_audio_is_decoded_without_losing_samples() -> Result<(), SpeechRecognitionError> {
    let decoded_samples = Arc::new(AtomicUsize::new(0));
    let recognizer = WhisperSpeechRecognizer::with_decoder(Arc::new(FakeDecoder {
        calls: Arc::new(AtomicUsize::new(0)),
        decoded_samples: Arc::clone(&decoded_samples),
    }));
    let partials = Arc::new(AtomicUsize::new(0));
    let captured = Arc::clone(&partials);
    let session = recognizer.start(
        SpeechRecognitionHints::default(),
        Arc::new(move |_| {
            captured.fetch_add(1, Ordering::Relaxed);
        }),
    )?;
    let total_samples = MAX_SEGMENT_SAMPLES + 1_600;
    session.push_audio(vec![1; total_samples])?;
    let transcript = session.finish()?;
    assert_eq!(total_samples, decoded_samples.load(Ordering::Relaxed));
    assert_eq!("segment segment", transcript);
    assert_eq!(0, partials.load(Ordering::Relaxed));
    Ok(())
}

#[test]
fn cancellation_does_not_decode_buffered_audio() -> Result<(), SpeechRecognitionError> {
    let calls = Arc::new(AtomicUsize::new(0));
    let recognizer = WhisperSpeechRecognizer::with_decoder(Arc::new(FakeDecoder {
        calls: Arc::clone(&calls),
        decoded_samples: Arc::new(AtomicUsize::new(0)),
    }));
    let session = recognizer.start(SpeechRecognitionHints::default(), Arc::new(|_| {}))?;
    session.push_audio(vec![1; 1_600])?;
    session.cancel();
    assert_eq!(0, calls.load(Ordering::Relaxed));
    Ok(())
}

#[test]
fn empty_audio_is_rejected_without_decoding() -> Result<(), SpeechRecognitionError> {
    let calls = Arc::new(AtomicUsize::new(0));
    let recognizer = WhisperSpeechRecognizer::with_decoder(Arc::new(FakeDecoder {
        calls: Arc::clone(&calls),
        decoded_samples: Arc::new(AtomicUsize::new(0)),
    }));
    let session = recognizer.start(SpeechRecognitionHints::default(), Arc::new(|_| {}))?;
    assert!(matches!(
        session.finish(),
        Err(SpeechRecognitionError::Protocol(_))
    ));
    assert_eq!(0, calls.load(Ordering::Relaxed));
    Ok(())
}

#[test]
fn dictionary_snapshot_reaches_every_segment_without_leaking_between_sessions()
-> Result<(), SpeechRecognitionError> {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let recognizer = WhisperSpeechRecognizer::with_decoder(Arc::new(PromptDecoder {
        prompts: Arc::clone(&prompts),
    }));
    let hinted = recognizer.start(
        SpeechRecognitionHints::from_terms(vec!["Saymore".to_owned(), "思源黑体".to_owned()]),
        Arc::new(|_| {}),
    )?;
    let plain = recognizer.start(SpeechRecognitionHints::default(), Arc::new(|_| {}))?;
    hinted.push_audio(vec![1; MAX_SEGMENT_SAMPLES + 1_600])?;
    hinted.finish()?;
    plain.push_audio(vec![1; 1_600])?;
    plain.finish()?;
    assert_eq!(
        vec!["Saymore, 思源黑体", "Saymore, 思源黑体", ""],
        *prompts.lock().map_err(lock_error)?
    );
    Ok(())
}

#[test]
fn missing_and_legacy_onnx_models_are_not_loaded() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    fs::write(directory.path().join("turbo-encoder.int8.onnx"), b"old")?;
    assert!(matches!(
        WhisperSpeechRecognizer::load(directory.path()),
        Err(SpeechRecognitionError::NotConfigured)
    ));
    Ok(())
}

#[test]
fn joins_chinese_segments_without_inserting_spaces() {
    let mut transcript = "你好".to_owned();
    append_segment(&mut transcript, "世界");
    assert_eq!("你好世界", transcript);
}

struct PromptDecoder {
    prompts: Arc<Mutex<Vec<String>>>,
}

impl WhisperDecoder for PromptDecoder {
    fn decode(&self, _samples: &[i16], prompt: &str) -> Result<String, SpeechRecognitionError> {
        self.prompts
            .lock()
            .map_err(lock_error)?
            .push(prompt.to_owned());
        Ok("segment".to_owned())
    }
}

struct FakeDecoder {
    calls: Arc<AtomicUsize>,
    decoded_samples: Arc<AtomicUsize>,
}

impl WhisperDecoder for FakeDecoder {
    fn decode(&self, samples: &[i16], _prompt: &str) -> Result<String, SpeechRecognitionError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.decoded_samples
            .fetch_add(samples.len(), Ordering::Relaxed);
        Ok("segment".to_owned())
    }
}
