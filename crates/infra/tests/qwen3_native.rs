#![allow(clippy::panic_in_result_fn)]

//! Opt-in native acceptance; the caller supplies the verified GGUF installation.
use std::{path::PathBuf, sync::Arc};
use template_app::{SpeechRecognitionHints, StreamingSpeechRecognizer};
use template_infra::Qwen3AsrSpeechRecognizer;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
#[ignore = "requires SAYMORE_QWEN_MODEL_DIR with the pinned Q8_0 GGUF"]
fn native_sessions_preserve_text_vocabulary_isolation_and_cancellation() -> TestResult {
    let directory = PathBuf::from(
        std::env::var_os("SAYMORE_QWEN_MODEL_DIR")
            .ok_or("set SAYMORE_QWEN_MODEL_DIR to the verified GGUF directory")?,
    );
    let model = Qwen3AsrSpeechRecognizer::load(&directory)?;
    let audio = include_bytes!("../../../apps/desktop/assets/asr-test/standard-zh.pcm")
        .as_chunks::<2>()
        .0
        .iter()
        .map(|bytes| i16::from_le_bytes(*bytes))
        .collect::<Vec<_>>();
    let transcribe = |hints| -> Result<String, template_app::SpeechRecognitionError> {
        let session = model.start(hints, Arc::new(|_| {}))?;
        session.push_audio(audio.clone())?;
        session.finish()
    };
    let first = transcribe(SpeechRecognitionHints::default())?;
    assert!(!first.trim().is_empty());
    let hinted = transcribe(SpeechRecognitionHints::from_terms(vec![
        "Saymore".into(),
        "声墨".into(),
    ]))?;
    assert!(!hinted.trim().is_empty());
    let cancelled = model.start(SpeechRecognitionHints::default(), Arc::new(|_| {}))?;
    cancelled.push_audio(audio.clone())?;
    cancelled.cancel();
    assert_eq!(first, transcribe(SpeechRecognitionHints::default())?);
    assert!(model.resident_memory_bytes().is_some_and(|bytes| bytes > 0));
    let empty = model.start(SpeechRecognitionHints::default(), Arc::new(|_| {}))?;
    assert!(empty.finish().is_err());
    let silence = model.start(SpeechRecognitionHints::default(), Arc::new(|_| {}))?;
    silence.push_audio(vec![0; 16_000])?;
    assert!(silence.finish().is_err());
    Ok(())
}

#[test]
#[ignore = "requires SAYMORE_QWEN_MODEL_DIR with the pinned Q8_0 GGUF"]
fn streaming_emits_before_stop_and_keeps_sessions_separate() -> TestResult {
    use std::{sync::mpsc, time::Duration};
    use template_app::QwenRecognitionMode;
    let directory = PathBuf::from(
        std::env::var_os("SAYMORE_QWEN_MODEL_DIR").ok_or("model directory required")?,
    );
    let model = Qwen3AsrSpeechRecognizer::load(&directory)?;
    let audio = include_bytes!("../../../apps/desktop/assets/asr-test/standard-zh.pcm")
        .as_chunks::<2>()
        .0
        .iter()
        .map(|bytes| i16::from_le_bytes(*bytes))
        .collect::<Vec<_>>();
    let (send, received) = mpsc::channel();
    let session = model.start_with_mode(
        QwenRecognitionMode::Streaming,
        SpeechRecognitionHints::default(),
        Arc::new(move |text| {
            let _ = send.send(text);
        }),
    )?;
    session.push_audio(audio[..32_000].to_vec())?;
    assert!(!received.recv_timeout(Duration::from_secs(30))?.is_empty());
    let mut rest = audio[32_000..].to_vec();
    rest.extend_from_slice(&audio);
    session.push_audio(rest)?;
    let text = session.finish()?;
    assert_eq!(2, text.matches("可以通过语音进行人机交互").count());
    for mode in [
        QwenRecognitionMode::NonStreaming,
        QwenRecognitionMode::Streaming,
    ] {
        let session =
            model.start_with_mode(mode, SpeechRecognitionHints::default(), Arc::new(|_| {}))?;
        session.push_audio(audio.clone())?;
        assert_eq!("可以通过语音进行人机交互。", session.finish()?);
    }
    Ok(())
}
