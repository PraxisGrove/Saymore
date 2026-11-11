use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use template_app::{
    DictationSessionId, DictionaryStore, OwnedRecognition, PcmRecording,
    RestoredRecordingTranscriber, SpeechRecognitionError, SpeechRecognitionHints,
    StreamingSpeechRecognizer,
};
#[cfg(target_os = "macos")]
use template_infra::MacOsSpeechRecognizer;
use template_infra::{
    JsonSettingsStore, OpenAiCompatibleSpeechRecognizer, QWEN3_ASR_MODEL_ID,
    QWEN3_ASR_MODEL_REVISION, Qwen3AsrSpeechRecognizer, SENSE_VOICE_MODEL_ID,
    SENSE_VOICE_MODEL_REVISION, VolcengineSpeechRecognizer, WHISPER_MODEL_ID,
    WHISPER_MODEL_REVISION, WhisperSpeechRecognizer, current_process_resident_memory_bytes,
};

mod additional_models;
use additional_models::LoadedLocal;
use template_infra::{
    MOONSHINE_MODEL_ID, MOONSHINE_MODEL_REVISION, SEACO_MODEL_ID, SEACO_MODEL_REVISION,
    ZIPFORMER_MODEL_ID, ZIPFORMER_MODEL_REVISION,
};
pub(crate) mod self_test;
mod sense_voice;

use sense_voice::LoadedSenseVoice;

pub struct AsrSessionController {
    settings: Arc<JsonSettingsStore>,
    dictionary: Arc<dyn DictionaryStore>,
    active: Mutex<Option<OwnedRecognition>>,
    whisper_model_directory: PathBuf,
    whisper: Mutex<Option<LoadedWhisper>>,
    qwen3_model_directory: PathBuf,
    qwen3: Mutex<Option<LoadedQwen3>>,
    sense_voice_model_directory: PathBuf,
    sense_voice: Mutex<Option<LoadedSenseVoice>>,
    seaco_model_directory: PathBuf,
    seaco: Mutex<Option<LoadedLocal>>,
    zipformer_model_directory: PathBuf,
    zipformer: Mutex<Option<LoadedLocal>>,
    moonshine_model_directory: PathBuf,
    moonshine: Mutex<Option<LoadedLocal>>,
}

struct LoadedWhisper {
    recognizer: Arc<WhisperSpeechRecognizer>,
    resident_memory_bytes: Option<u64>,
}

struct LoadedQwen3 {
    recognizer: Arc<Qwen3AsrSpeechRecognizer>,
    resident_memory_bytes: Option<u64>,
}

impl AsrSessionController {
    pub fn new(
        settings: Arc<JsonSettingsStore>,
        dictionary: Arc<dyn DictionaryStore>,
        models_directory: PathBuf,
    ) -> Self {
        Self {
            settings,
            dictionary,
            active: Mutex::new(None),
            whisper_model_directory: models_directory
                .join(WHISPER_MODEL_ID)
                .join(WHISPER_MODEL_REVISION),
            whisper: Mutex::new(None),
            qwen3_model_directory: models_directory
                .join(QWEN3_ASR_MODEL_ID)
                .join(QWEN3_ASR_MODEL_REVISION),
            qwen3: Mutex::new(None),
            sense_voice_model_directory: models_directory
                .join(SENSE_VOICE_MODEL_ID)
                .join(SENSE_VOICE_MODEL_REVISION),
            sense_voice: Mutex::new(None),
            seaco_model_directory: models_directory
                .join(SEACO_MODEL_ID)
                .join(SEACO_MODEL_REVISION),
            seaco: Mutex::new(None),
            zipformer_model_directory: models_directory
                .join(ZIPFORMER_MODEL_ID)
                .join(ZIPFORMER_MODEL_REVISION),
            zipformer: Mutex::new(None),
            moonshine_model_directory: models_directory
                .join(MOONSHINE_MODEL_ID)
                .join(MOONSHINE_MODEL_REVISION),
            moonshine: Mutex::new(None),
        }
    }

    pub fn prepare_whisper(&self) -> Result<Option<u64>, SpeechRecognitionError> {
        let loaded = self.whisper_runtime()?;
        ensure_self_test(loaded.recognizer.as_ref())?;
        Ok(loaded.resident_memory_bytes)
    }

    pub fn clear_whisper(&self) {
        if let Ok(mut recognizer) = self.whisper.lock() {
            recognizer.take();
        }
    }

    pub fn prepare_qwen3(&self) -> Result<Option<u64>, SpeechRecognitionError> {
        let loaded = self.qwen3_runtime()?;
        ensure_self_test(loaded.recognizer.as_ref())?;
        Ok(loaded.resident_memory_bytes)
    }

    pub fn clear_qwen3(&self) {
        if let Ok(mut recognizer) = self.qwen3.lock() {
            recognizer.take();
        }
    }

    pub fn start(
        &self,
        id: DictationSessionId,
        on_partial: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Result<(), SpeechRecognitionError> {
        let (settings, catalog) = self
            .settings
            .load_settings_snapshot()
            .map_err(|error| SpeechRecognitionError::Protocol(error.to_string()))?;
        let macos_speech_active = catalog.macos_speech_is_active();
        let whisper_active = catalog.whisper_is_active();
        let qwen3_active = catalog.qwen3_asr_is_active();
        let sense_voice_active = catalog.sense_voice_is_active();
        let seaco_active = catalog.seaco_is_active();
        let zipformer_active = catalog.zipformer_is_active();
        let moonshine_active = catalog.moonshine_is_active();
        if !seaco_active
            && !zipformer_active
            && !moonshine_active
            && !sense_voice_active
            && !qwen3_active
            && !whisper_active
            && !macos_speech_active
            && !settings.asr.volcengine.enabled
            && !settings.asr.openai_compatible.enabled
        {
            return Err(SpeechRecognitionError::NotConfigured);
        }
        let hints = self.dictionary_hints(&id);
        let mut active = self
            .active
            .lock()
            .map_err(|_| SpeechRecognitionError::Transport("ASR lock was poisoned".to_owned()))?;
        if active.is_some() {
            return Err(SpeechRecognitionError::Transport(
                "ASR session is already active".to_owned(),
            ));
        }
        let session = if seaco_active {
            self.seaco_recognizer()?.start(hints, on_partial)?
        } else if zipformer_active {
            self.zipformer_recognizer()?.start(hints, on_partial)?
        } else if moonshine_active {
            self.moonshine_recognizer()?.start(hints, on_partial)?
        } else if sense_voice_active {
            self.sense_voice_recognizer()?.start(hints, on_partial)?
        } else if qwen3_active {
            self.qwen3_recognizer()?.start(hints, on_partial)?
        } else if whisper_active {
            self.whisper_recognizer()?.start(hints, on_partial)?
        } else if macos_speech_active {
            #[cfg(target_os = "macos")]
            {
                MacOsSpeechRecognizer::new().start(hints, on_partial)?
            }
            #[cfg(not(target_os = "macos"))]
            {
                return Err(SpeechRecognitionError::NotConfigured);
            }
        } else if settings.asr.openai_compatible.enabled {
            OpenAiCompatibleSpeechRecognizer::new(settings.asr.openai_compatible)?
                .start(hints, on_partial)?
        } else {
            VolcengineSpeechRecognizer::new(settings.asr.volcengine)?.start(hints, on_partial)?
        };
        *active = Some(OwnedRecognition::new(session));
        tracing::info!(
            target: "saymore::diagnostics",
            event = "asr.session_started",
            dictation_id = %id
        );
        Ok(())
    }

    fn dictionary_hints(&self, id: &DictationSessionId) -> SpeechRecognitionHints {
        match self.dictionary.list_dictionary() {
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
        }
    }

    pub fn whisper_recognizer(
        &self,
    ) -> Result<Arc<WhisperSpeechRecognizer>, SpeechRecognitionError> {
        self.whisper_runtime().map(|loaded| loaded.recognizer)
    }

    pub fn qwen3_recognizer(
        &self,
    ) -> Result<Arc<Qwen3AsrSpeechRecognizer>, SpeechRecognitionError> {
        self.qwen3_runtime().map(|loaded| loaded.recognizer)
    }

    fn whisper_runtime(&self) -> Result<LoadedWhisper, SpeechRecognitionError> {
        let mut recognizer = self.whisper.lock().map_err(|_| {
            SpeechRecognitionError::Transport("Whisper cache lock was poisoned".to_owned())
        })?;
        if let Some(loaded) = recognizer.as_ref() {
            return Ok(loaded.clone());
        }
        let resident_before = current_process_resident_memory_bytes();
        let loaded = Arc::new(WhisperSpeechRecognizer::load(
            &self.whisper_model_directory,
        )?);
        let resident_memory_bytes =
            resident_memory_delta(resident_before, current_process_resident_memory_bytes());
        let loaded = LoadedWhisper {
            recognizer: loaded,
            resident_memory_bytes,
        };
        *recognizer = Some(loaded.clone());
        Ok(loaded)
    }

    fn qwen3_runtime(&self) -> Result<LoadedQwen3, SpeechRecognitionError> {
        let mut recognizer = self.qwen3.lock().map_err(|_| {
            SpeechRecognitionError::Transport("Qwen3-ASR cache lock was poisoned".to_owned())
        })?;
        if let Some(loaded) = recognizer.as_ref() {
            return Ok(loaded.clone());
        }
        let resident_before = current_process_resident_memory_bytes();
        let loaded = Arc::new(Qwen3AsrSpeechRecognizer::load(&self.qwen3_model_directory)?);
        let resident_memory_bytes =
            resident_memory_delta(resident_before, current_process_resident_memory_bytes());
        let loaded = LoadedQwen3 {
            recognizer: loaded,
            resident_memory_bytes,
        };
        *recognizer = Some(loaded.clone());
        Ok(loaded)
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
        let result = self
            .active
            .lock()
            .map_err(|_| SpeechRecognitionError::Transport("ASR lock was poisoned".to_owned()))?
            .take()
            .ok_or_else(|| SpeechRecognitionError::Transport("ASR session is inactive".to_owned()));
        if result.is_ok() {
            tracing::info!(
                target: "saymore::diagnostics",
                event = "asr.session_finishing"
            );
        }
        result
    }

    pub fn cancel(&self) {
        if let Ok(mut active) = self.active.lock()
            && let Some(recognition) = active.take()
        {
            recognition.cancel();
            tracing::info!(
                target: "saymore::diagnostics",
                event = "asr.session_cancelled"
            );
        }
    }
}

impl Clone for LoadedWhisper {
    fn clone(&self) -> Self {
        Self {
            recognizer: Arc::clone(&self.recognizer),
            resident_memory_bytes: self.resident_memory_bytes,
        }
    }
}

impl Clone for LoadedQwen3 {
    fn clone(&self) -> Self {
        Self {
            recognizer: Arc::clone(&self.recognizer),
            resident_memory_bytes: self.resident_memory_bytes,
        }
    }
}

fn resident_memory_delta(before: Option<u64>, after: Option<u64>) -> Option<u64> {
    before
        .zip(after)
        .and_then(|(before, after)| after.checked_sub(before))
        .filter(|bytes| *bytes > 0)
}

fn ensure_self_test(
    recognizer: &dyn StreamingSpeechRecognizer,
) -> Result<(), SpeechRecognitionError> {
    let (_, result) = self_test::run(recognizer);
    result.map(|_| ())
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

#[cfg(test)]
mod tests {
    use super::resident_memory_delta;

    #[test]
    fn resident_memory_delta_requires_a_positive_complete_sample() {
        assert_eq!(Some(512), resident_memory_delta(Some(1_024), Some(1_536)));
        assert_eq!(None, resident_memory_delta(Some(1_024), Some(1_024)));
        assert_eq!(None, resident_memory_delta(Some(1_024), Some(512)));
        assert_eq!(None, resident_memory_delta(None, Some(1_536)));
    }
}
