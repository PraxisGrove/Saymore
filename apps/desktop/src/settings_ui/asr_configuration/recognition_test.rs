use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use template_app::{SpeechRecognitionError, SpeechRecognitionHints, StreamingSpeechRecognizer};
#[cfg(target_os = "macos")]
use template_infra::MacOsSpeechRecognizer;
use template_infra::{OpenAiCompatibleSpeechRecognizer, VolcengineSpeechRecognizer};

use super::AsrCandidate;

const STANDARD_AUDIO: &[u8] = include_bytes!("../../../assets/asr-test/standard-zh.pcm");
const EXPECTED_SAMPLE_COUNT: usize = 61_744;

pub(super) struct RecognitionTestAttempt {
    pub(super) elapsed: Duration,
    pub(super) result: Result<String, SpeechRecognitionError>,
}

pub(super) fn run(candidate: &AsrCandidate) -> RecognitionTestAttempt {
    let Ok(samples) = standard_audio_samples() else {
        return RecognitionTestAttempt {
            elapsed: Duration::ZERO,
            result: Err(SpeechRecognitionError::Protocol(
                "standard recognition audio is invalid".to_owned(),
            )),
        };
    };
    let started = Instant::now();
    let result = recognize(candidate, samples);
    RecognitionTestAttempt {
        result,
        elapsed: started.elapsed(),
    }
}

fn recognize(
    candidate: &AsrCandidate,
    samples: Vec<i16>,
) -> Result<String, SpeechRecognitionError> {
    match candidate {
        AsrCandidate::Volcengine(settings) => {
            let recognizer = VolcengineSpeechRecognizer::new(settings.clone())?;
            recognize_with(&recognizer, samples)
        }
        AsrCandidate::Custom(settings) => {
            let recognizer = OpenAiCompatibleSpeechRecognizer::new(settings.clone())?;
            recognize_with(&recognizer, samples)
        }
    }
}

#[cfg(target_os = "macos")]
pub(super) fn run_macos() -> RecognitionTestAttempt {
    let Ok(samples) = standard_audio_samples() else {
        return RecognitionTestAttempt {
            elapsed: Duration::ZERO,
            result: Err(SpeechRecognitionError::Protocol(
                "standard recognition audio is invalid".to_owned(),
            )),
        };
    };
    let started = Instant::now();
    let result = recognize_with(&MacOsSpeechRecognizer::new(), samples);
    RecognitionTestAttempt {
        result,
        elapsed: started.elapsed(),
    }
}

fn recognize_with(
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
            "recognition test returned no text".to_owned(),
        ));
    }
    Ok(transcript)
}

fn standard_audio_samples() -> Result<Vec<i16>, SpeechRecognitionError> {
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
