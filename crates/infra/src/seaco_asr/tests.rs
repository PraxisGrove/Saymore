use std::{
    error::Error,
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

use super::*;

struct FakeDecoder {
    calls: AtomicUsize,
}

impl SeacoDecoder for FakeDecoder {
    fn embed(&self, hotwords: &Hotwords) -> Result<Vec<f32>, SpeechRecognitionError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Ok(vec![hotwords.matrix[0] as f32])
    }

    fn decode(&self, samples: &[i16], bias: &[f32]) -> Result<String, SpeechRecognitionError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Ok(if bias == [3.0] {
            "魔搭"
        } else if samples.first() == Some(&2) {
            "world"
        } else {
            "hello"
        }
        .to_owned())
    }
}

fn fake_recognizer() -> Result<(SeacoSpeechRecognizer, Arc<FakeDecoder>), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let mut tokens: Vec<String> = (0..8_404).map(|index| format!("token{index}")).collect();
    tokens[3] = "魔".to_owned();
    tokens[4] = "搭".to_owned();
    let tokens_file = directory.path().join("tokens.json");
    let segmentation_file = directory.path().join("seg_dict");
    fs::write(&tokens_file, serde_json::to_vec(&tokens)?)?;
    fs::write(&segmentation_file, "hello token5\n")?;
    let vocabulary = Arc::new(Vocabulary::load(&tokens_file, &segmentation_file)?);
    let decoder = Arc::new(FakeDecoder {
        calls: AtomicUsize::new(0),
    });
    Ok((
        SeacoSpeechRecognizer {
            decoder: decoder.clone(),
            vocabulary,
        },
        decoder,
    ))
}

#[test]
fn sessions_keep_independent_hint_snapshots_and_cancel_without_inference()
-> Result<(), Box<dyn Error>> {
    let (recognizer, decoder) = fake_recognizer()?;
    let baseline = recognizer.start(SpeechRecognitionHints::default(), Arc::new(|_| {}))?;
    let hinted = recognizer.start(
        SpeechRecognitionHints::from_terms(vec!["魔搭".to_owned()]),
        Arc::new(|_| {}),
    )?;
    let cancelled = recognizer.start(SpeechRecognitionHints::default(), Arc::new(|_| {}))?;
    cancelled.push_audio(vec![1; 1_600])?;
    cancelled.cancel();
    assert_eq!(0, decoder.calls.load(Ordering::Relaxed));
    baseline.push_audio(vec![1; 1_600])?;
    hinted.push_audio(vec![1; 1_600])?;
    assert_eq!("魔搭", hinted.finish()?);
    assert_eq!("hello", baseline.finish()?);
    assert_eq!(4, decoder.calls.load(Ordering::Relaxed));
    Ok(())
}

#[test]
fn ordered_audio_chunks_join_english_segments_and_empty_audio_does_not_decode()
-> Result<(), Box<dyn Error>> {
    let (recognizer, decoder) = fake_recognizer()?;
    let empty = recognizer.start(SpeechRecognitionHints::default(), Arc::new(|_| {}))?;
    assert!(empty.finish().is_err());
    assert_eq!(0, decoder.calls.load(Ordering::Relaxed));
    let session = recognizer.start(SpeechRecognitionHints::default(), Arc::new(|_| {}))?;
    session.push_audio(vec![1; MAX_SEGMENT_SAMPLES - 10])?;
    session.push_audio(vec![2; 1_610])?;
    assert_eq!("hello world", session.finish()?);
    assert_eq!(3, decoder.calls.load(Ordering::Relaxed));
    Ok(())
}

/// Requires the locally validated exact FP32 export and non-sensitive moda PCM.
/// Run with SAYMORE_SEACO_MODEL_DIR and SAYMORE_SEACO_FIXTURE_PCM, --ignored.
#[test]
#[ignore = "requires the exact local SeACo FP32 export and 16 kHz moda fixture"]
fn native_dictionary_changes_decoding_and_restores_baseline() -> Result<(), Box<dyn Error>> {
    let model = PathBuf::from(std::env::var("SAYMORE_SEACO_MODEL_DIR")?);
    let fixture = fs::read(std::env::var("SAYMORE_SEACO_FIXTURE_PCM")?)?;
    assert!(fixture.len().is_multiple_of(2));
    let samples: Vec<i16> = fixture
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| i16::from_le_bytes(*pair))
        .collect();
    let recognizer = SeacoSpeechRecognizer::load(&model)?;
    let decode = |terms: &[&str]| -> Result<String, SpeechRecognitionError> {
        let hints = SpeechRecognitionHints::from_terms(
            terms.iter().map(|term| (*term).to_owned()).collect(),
        );
        let session = recognizer.start(hints, Arc::new(|_| {}))?;
        for chunk in samples.chunks(1_600) {
            session.push_audio(chunk.to_vec())?;
        }
        session.finish()
    };
    let baseline = decode(&[])?;
    assert_eq!("欢迎来到摩哒社区达摩院的张伟正在介绍语音识别", baseline);
    let hinted = decode(&["魔搭", "达摩院", "张伟"])?;
    assert_eq!("欢迎来到魔搭社区达摩院的张伟正在介绍语音识别", hinted);
    assert_eq!(baseline, decode(&[])?);
    assert_eq!(hinted, decode(&["张伟", "达摩院", "魔搭"])?);
    assert_eq!(baseline, decode(&[])?);
    let cancelled = recognizer.start(
        SpeechRecognitionHints::from_terms(vec!["魔搭".to_owned()]),
        Arc::new(|_| {}),
    )?;
    cancelled.push_audio(samples)?;
    cancelled.cancel();
    let silent = recognizer.start(SpeechRecognitionHints::default(), Arc::new(|_| {}))?;
    silent.push_audio(vec![0; 16_000])?;
    assert!(silent.finish().is_err());
    Ok(())
}
