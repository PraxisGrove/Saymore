use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

#[derive(Default)]
struct FakeDecoder {
    segments: Mutex<Vec<Vec<i16>>>,
    text: &'static str,
}

impl MoonshineDecoder for FakeDecoder {
    fn decode(&self, samples: &[i16]) -> Result<String, SpeechRecognitionError> {
        self.segments
            .lock()
            .map_err(lock_error)?
            .push(samples.to_vec());
        Ok(self.text.to_owned())
    }
}

#[test]
fn buffers_ordered_chunks_and_segments_long_audio_without_fabricated_partials()
-> Result<(), SpeechRecognitionError> {
    let decoder = Arc::new(FakeDecoder {
        text: " English sentence. ",
        ..Default::default()
    });
    let recognizer = MoonshineSpeechRecognizer {
        decoder: decoder.clone(),
    };
    let partials = Arc::new(AtomicUsize::new(0));
    let captured = Arc::clone(&partials);
    let session = recognizer.start(
        SpeechRecognitionHints::from_terms(vec!["ignored".to_owned()]),
        Arc::new(move |_| {
            captured.fetch_add(1, Ordering::Relaxed);
        }),
    )?;
    let mut samples = vec![1; MAX_SEGMENT_SAMPLES - 1];
    samples.extend([2, 3, 4]);
    for chunk in samples.chunks(9_600) {
        session.push_audio(chunk.to_vec())?;
    }
    assert!(decoder.segments.lock().map_err(lock_error)?.is_empty());
    assert_eq!("English sentence. English sentence.", session.finish()?);
    assert_eq!(
        vec![samples[..MAX_SEGMENT_SAMPLES].to_vec(), vec![3, 4]],
        *decoder.segments.lock().map_err(lock_error)?
    );
    assert_eq!(0, partials.load(Ordering::Relaxed));
    Ok(())
}

#[test]
fn empty_and_cancelled_sessions_never_decode_or_contaminate_the_next_session()
-> Result<(), SpeechRecognitionError> {
    let decoder = Arc::new(FakeDecoder {
        text: "fresh session",
        ..Default::default()
    });
    let recognizer = MoonshineSpeechRecognizer {
        decoder: decoder.clone(),
    };
    let empty = recognizer.start(SpeechRecognitionHints::default(), Arc::new(|_| {}))?;
    empty.push_audio(Vec::new())?;
    assert!(matches!(
        empty.finish(),
        Err(SpeechRecognitionError::Protocol(_))
    ));
    let cancelled = recognizer.start(SpeechRecognitionHints::default(), Arc::new(|_| {}))?;
    cancelled.push_audio(vec![5, 6, 7])?;
    cancelled.cancel();
    assert!(decoder.segments.lock().map_err(lock_error)?.is_empty());
    let next = recognizer.start(SpeechRecognitionHints::default(), Arc::new(|_| {}))?;
    next.push_audio(vec![8, 9])?;
    assert_eq!("fresh session", next.finish()?);
    assert_eq!(
        vec![vec![8, 9]],
        *decoder.segments.lock().map_err(lock_error)?
    );
    Ok(())
}

#[test]
fn rejects_a_whitespace_only_native_transcript() -> Result<(), SpeechRecognitionError> {
    let recognizer = MoonshineSpeechRecognizer {
        decoder: Arc::new(FakeDecoder {
            text: " \n ",
            ..Default::default()
        }),
    };
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
        MoonshineSpeechRecognizer::load(directory.path()),
        Err(SpeechRecognitionError::NotConfigured)
    ));
    for (name, bytes) in MODEL_FILES {
        fs::File::create(directory.path().join(name))?.set_len(bytes)?;
    }
    assert!(validate_files(directory.path()).is_ok());
    fs::File::create(directory.path().join("tokens.txt"))?.set_len(1)?;
    assert!(matches!(
        MoonshineSpeechRecognizer::load(directory.path()),
        Err(SpeechRecognitionError::Protocol(_))
    ));
    Ok(())
}
