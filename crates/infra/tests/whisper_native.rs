#![allow(clippy::panic_in_result_fn)]

use std::{
    path::PathBuf,
    sync::{Arc, mpsc},
    time::Duration,
};
use template_app::{SpeechRecognitionHints, StreamingSpeechRecognizer, WhisperRecognitionMode};
use template_infra::WhisperSpeechRecognizer;

#[test]
#[ignore = "requires SAYMORE_WHISPER_MODEL_DIR with the pinned Q8_0 model"]
fn native_preview_finalization_and_session_isolation() -> Result<(), Box<dyn std::error::Error>> {
    let directory = PathBuf::from(
        std::env::var_os("SAYMORE_WHISPER_MODEL_DIR").ok_or("model directory required")?,
    );
    let model = WhisperSpeechRecognizer::load(&directory)?;
    let audio = include_bytes!("../../../apps/desktop/assets/asr-test/standard-zh.pcm")
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| i16::from_le_bytes(*b))
        .collect::<Vec<_>>();
    let baseline = model.start(SpeechRecognitionHints::default(), Arc::new(|_| {}))?;
    baseline.push_audio(audio.clone())?;
    let expected = baseline.finish()?;
    assert!(!expected.is_empty());
    let (tx, rx) = mpsc::channel();
    let session = model.start_with_mode(
        WhisperRecognitionMode::Streaming,
        SpeechRecognitionHints::default(),
        Arc::new(move |text| {
            let _ = tx.send(text);
        }),
    )?;
    session.push_audio(audio[..32_000].to_vec())?;
    assert!(!rx.recv_timeout(Duration::from_secs(30))?.is_empty());
    session.push_audio(audio[32_000..].to_vec())?;
    assert_eq!(expected, session.finish()?);
    let hinted = model.start_with_mode(
        WhisperRecognitionMode::Streaming,
        SpeechRecognitionHints::from_terms(vec!["Saymore".into(), "声墨".into()]),
        Arc::new(|_| {}),
    )?;
    hinted.push_audio(audio.clone())?;
    assert!(!hinted.finish()?.is_empty());
    let cancelled = model.start_with_mode(
        WhisperRecognitionMode::Streaming,
        SpeechRecognitionHints::default(),
        Arc::new(|_| {}),
    )?;
    cancelled.push_audio(audio.clone())?;
    cancelled.cancel();
    let next = model.start(SpeechRecognitionHints::default(), Arc::new(|_| {}))?;
    next.push_audio(audio)?;
    assert_eq!(expected, next.finish()?);
    Ok(())
}
