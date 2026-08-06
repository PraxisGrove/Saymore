use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use template_app::{
    AsrProviderConfiguration, SpeechRecognitionError, SpeechRecognitionHints,
    StreamingSpeechRecognizer,
};
#[cfg(target_os = "macos")]
use template_infra::MacOsSpeechRecognizer;
use template_infra::{OpenAiCompatibleSpeechRecognizer, VolcengineSpeechRecognizer};

pub(in crate::settings_ui) struct RecognitionTestAttempt {
    #[cfg_attr(
        not(target_os = "macos"),
        expect(
            dead_code,
            reason = "only the macOS system recognition test displays it"
        )
    )]
    pub(in crate::settings_ui) elapsed: Duration,
    pub(in crate::settings_ui) result: Result<String, SpeechRecognitionError>,
}

pub(in crate::settings_ui) fn run(candidate: &AsrProviderConfiguration) -> RecognitionTestAttempt {
    let Ok(samples) = crate::asr_runtime::self_test::standard_audio_samples() else {
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
    candidate: &AsrProviderConfiguration,
    samples: Vec<i16>,
) -> Result<String, SpeechRecognitionError> {
    match candidate {
        AsrProviderConfiguration::Volcengine(settings) => {
            let recognizer = VolcengineSpeechRecognizer::new(settings.clone())?;
            recognize_with(&recognizer, samples)
        }
        AsrProviderConfiguration::OpenAiCompatible(settings) => {
            let recognizer = OpenAiCompatibleSpeechRecognizer::new(settings.clone())?;
            recognize_with(&recognizer, samples)
        }
    }
}

#[cfg(target_os = "macos")]
pub(super) fn run_macos() -> RecognitionTestAttempt {
    let (elapsed, result) = crate::asr_runtime::self_test::run(&MacOsSpeechRecognizer::new());
    RecognitionTestAttempt { result, elapsed }
}

pub(super) fn recognize_with(
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
