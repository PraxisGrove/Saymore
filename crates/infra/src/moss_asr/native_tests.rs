#![allow(clippy::panic_in_result_fn)]

use super::*;

use std::{path::PathBuf, sync::Arc};

#[test]
#[ignore = "requires the three pinned MOSS files in SAYMORE_MOSS_MODEL_DIR (about 10 GB runtime memory)"]
fn pinned_model_transcribes_consecutive_sessions_long_audio_and_rejects_silence()
-> Result<(), SpeechRecognitionError> {
    let directory = std::env::var_os("SAYMORE_MOSS_MODEL_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| protocol("SAYMORE_MOSS_MODEL_DIR is required"))?;
    let recognizer = MossSpeechRecognizer::load(&directory)?;
    let fixture = include_bytes!("../../../../apps/desktop/assets/asr-test/standard-zh.pcm");
    let samples: Vec<_> = fixture
        .as_chunks::<2>()
        .0
        .iter()
        .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]))
        .collect();
    let expected = "可以通过语音进行人机交互。";
    for hints in [
        SpeechRecognitionHints::default(),
        SpeechRecognitionHints::from_terms(vec!["人机交互".into(), "Saymore".into()]),
    ] {
        let session = recognizer.start(hints, Arc::new(|_| {}))?;
        session.push_audio(samples.clone())?;
        assert_eq!(expected, session.finish()?);
    }
    let mut long = samples.clone();
    long.resize(SEGMENT_SAMPLES, 0);
    long.extend_from_slice(&samples);
    let session = recognizer.start(SpeechRecognitionHints::default(), Arc::new(|_| {}))?;
    session.push_audio(long)?;
    assert_eq!(expected.repeat(2), session.finish()?);
    let silent = recognizer.start(SpeechRecognitionHints::default(), Arc::new(|_| {}))?;
    silent.push_audio(vec![0; 16_000])?;
    assert!(silent.finish().is_err());
    Ok(())
}
