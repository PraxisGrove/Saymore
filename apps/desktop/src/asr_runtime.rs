use std::sync::{Arc, Mutex};

use template_app::{
    DictationSessionId, DictionaryStore, OwnedRecognition, PcmRecording,
    RestoredRecordingTranscriber, SettingsStore, SpeechRecognitionError, SpeechRecognitionHints,
    StreamingSpeechRecognizer,
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
        id: DictationSessionId,
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
                    dictation_id = %id,
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

impl RestoredRecordingTranscriber for AsrSessionController {
    fn transcribe(
        &self,
        id: DictationSessionId,
        recording: &PcmRecording,
    ) -> Result<String, SpeechRecognitionError> {
        let result = (|| {
            self.start(id, Arc::new(|_| {}))?;
            for chunk in recording.samples.chunks(1_600) {
                self.push_audio(chunk.to_vec())?;
            }
            self.take()?.finish()
        })();
        if result.is_err() {
            self.cancel();
        }
        result
    }
}
