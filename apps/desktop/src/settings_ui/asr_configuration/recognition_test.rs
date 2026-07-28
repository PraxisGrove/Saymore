use std::time::{Duration, Instant};

use template_app::SpeechRecognitionError;
use template_infra::{OpenAiCompatibleSpeechRecognizer, VolcengineSpeechRecognizer};

use super::AsrCandidate;

const STANDARD_AUDIO: &[u8] = include_bytes!("../../../assets/asr-test/standard-zh.pcm");
const EXPECTED_SAMPLE_COUNT: usize = 49_536;

pub(super) struct RecognitionTestAttempt {
    pub(super) elapsed: Duration,
    pub(super) result: Result<(), SpeechRecognitionError>,
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

fn recognize(candidate: &AsrCandidate, samples: Vec<i16>) -> Result<(), SpeechRecognitionError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| SpeechRecognitionError::Transport(error.to_string()))?;
    match candidate {
        AsrCandidate::Volcengine(settings) => {
            let recognizer = VolcengineSpeechRecognizer::new(settings.clone())?;
            runtime.block_on(recognizer.test_audio(samples))
        }
        AsrCandidate::Custom(settings) => {
            let recognizer = OpenAiCompatibleSpeechRecognizer::new(settings.clone())?;
            runtime.block_on(recognizer.test_audio(samples))
        }
    }
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
    fn bundled_audio_is_three_seconds_of_non_silent_pcm() {
        let Ok(samples) = standard_audio_samples() else {
            panic!("bundled test audio should be valid");
        };

        assert_eq!(EXPECTED_SAMPLE_COUNT, samples.len());
        assert!(samples.iter().any(|sample| sample.unsigned_abs() > 500));
    }
}
