use std::{sync::Arc, time::Instant};

use template_app::{SpeechRecognitionError, SpeechRecognitionHints, StreamingSpeechRecognizer};

const STANDARD_AUDIO: &[u8] = include_bytes!("../../assets/asr-test/standard-zh.pcm");
const EXPECTED_SAMPLE_COUNT: usize = 61_744;

pub(crate) fn run(
    recognizer: &dyn StreamingSpeechRecognizer,
) -> (std::time::Duration, Result<String, SpeechRecognitionError>) {
    let started = Instant::now();
    let result = standard_audio_samples().and_then(|samples| recognize(recognizer, samples));
    (started.elapsed(), result)
}

fn recognize(
    recognizer: &dyn StreamingSpeechRecognizer,
    samples: Vec<i16>,
) -> Result<String, SpeechRecognitionError> {
    let session = recognizer.start(SpeechRecognitionHints::default(), Arc::new(|_| {}))?;
    for chunk in samples.chunks(1_600) {
        session.push_audio(chunk.to_vec())?;
    }
    let transcript = session.finish()?;
    if transcript.trim().is_empty() {
        return Err(SpeechRecognitionError::Protocol(
            "recognition self-test returned no text".to_owned(),
        ));
    }
    Ok(transcript)
}

pub(crate) fn standard_audio_samples() -> Result<Vec<i16>, SpeechRecognitionError> {
    if STANDARD_AUDIO.len() != EXPECTED_SAMPLE_COUNT * 2 {
        return Err(SpeechRecognitionError::Protocol(
            "standard recognition audio has an invalid length".to_owned(),
        ));
    }
    Ok(STANDARD_AUDIO
        .chunks_exact(2)
        .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_audio_matches_the_expected_non_silent_pcm() {
        let Ok(samples) = standard_audio_samples() else {
            panic!("bundled test audio should be valid");
        };

        assert_eq!(EXPECTED_SAMPLE_COUNT, samples.len());
        assert!(samples.iter().any(|sample| sample.unsigned_abs() > 500));
    }
}
