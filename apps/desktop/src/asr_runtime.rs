use std::sync::{Arc, Mutex};

use template_app::{
    DictionaryStore, OwnedRecognition, SettingsStore, SpeechRecognitionError,
    SpeechRecognitionHints, StreamingSpeechRecognizer,
};
use template_infra::{
    JsonSettingsStore, OpenAiCompatibleSpeechRecognizer, VolcengineSpeechRecognizer,
};

pub struct AsrSessionController {
    settings: Arc<JsonSettingsStore>,
    dictionary: Arc<dyn DictionaryStore>,
    active: Mutex<Option<OwnedRecognition>>,
}

impl AsrSessionController {
    pub fn new(settings: Arc<JsonSettingsStore>, dictionary: Arc<dyn DictionaryStore>) -> Self {
        Self {
            settings,
            dictionary,
            active: Mutex::new(None),
        }
    }

    pub fn start(
        &self,
        on_partial: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Result<(), SpeechRecognitionError> {
        let settings = self
            .settings
            .load()
            .map_err(|error| SpeechRecognitionError::Protocol(error.to_string()))?;
        if !settings.asr.volcengine.enabled && !settings.asr.openai_compatible.enabled {
            return Err(SpeechRecognitionError::NotConfigured);
        }
        let hints = match self.dictionary.list_dictionary() {
            Ok(entries) => SpeechRecognitionHints::from_terms(
                entries.into_iter().map(|entry| entry.canonical).collect(),
            ),
            Err(error) => {
                tracing::warn!(
                    target: "saymore::diagnostics",
                    event = "asr.dictionary_hints_unavailable",
                    reason = %error
                );
                SpeechRecognitionHints::default()
            }
        };
        let mut active = self
            .active
            .lock()
            .map_err(|_| SpeechRecognitionError::Transport("ASR lock was poisoned".to_owned()))?;
        if active.is_some() {
            return Err(SpeechRecognitionError::Transport(
                "ASR session is already active".to_owned(),
            ));
        }
        let session = if settings.asr.openai_compatible.enabled {
            OpenAiCompatibleSpeechRecognizer::new(settings.asr.openai_compatible)?
                .start(hints, on_partial)?
        } else {
            VolcengineSpeechRecognizer::new(settings.asr.volcengine)?.start(hints, on_partial)?
        };
        *active = Some(OwnedRecognition::new(session));
        Ok(())
    }

    pub fn push_audio(&self, samples: Vec<i16>) -> Result<(), SpeechRecognitionError> {
        let mut active = self
            .active
            .lock()
            .map_err(|_| SpeechRecognitionError::Transport("ASR lock was poisoned".to_owned()))?;
        let recognition = active.as_mut().ok_or_else(|| {
            SpeechRecognitionError::Transport("ASR session is inactive".to_owned())
        })?;
        recognition.push_audio(samples)
    }

    pub fn take(&self) -> Result<OwnedRecognition, SpeechRecognitionError> {
        self.active
            .lock()
            .map_err(|_| SpeechRecognitionError::Transport("ASR lock was poisoned".to_owned()))?
            .take()
            .ok_or_else(|| SpeechRecognitionError::Transport("ASR session is inactive".to_owned()))
    }

    pub fn cancel(&self) {
        if let Ok(mut active) = self.active.lock()
            && let Some(recognition) = active.take()
        {
            recognition.cancel();
        }
    }
}

pub fn normalize_transcript(transcript: &str) -> String {
    transcript.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::normalize_transcript;

    #[test]
    fn normalizes_surrounding_and_repeated_whitespace() {
        assert_eq!(
            "你好 Saymore。",
            normalize_transcript("  你好   Saymore。\n")
        );
    }
}
