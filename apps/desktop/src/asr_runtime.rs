use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
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
    QWEN3_ASR_MODEL_REVISION, Qwen3AsrSpeechRecognizer, VolcengineSpeechRecognizer,
    WHISPER_MODEL_ID, WHISPER_MODEL_REVISION, WhisperSpeechRecognizer,
};

mod additional_models;
use template_infra::{
    SEACO_MODEL_ID, SEACO_MODEL_REVISION, SeacoSpeechRecognizer, ZIPFORMER_MODEL_ID,
    ZIPFORMER_MODEL_REVISION, ZipformerSpeechRecognizer,
};
pub(crate) mod self_test;

pub struct AsrSessionController {
    settings: Arc<JsonSettingsStore>,
    dictionary: Arc<dyn DictionaryStore>,
    active: Mutex<Option<OwnedRecognition>>,
    live_preview: AtomicBool,
    whisper_model_directory: PathBuf,
    whisper: Mutex<Option<Arc<WhisperSpeechRecognizer>>>,
    qwen3_model_directory: PathBuf,
    qwen3: Mutex<Option<Arc<Qwen3AsrSpeechRecognizer>>>,
    seaco_model_directory: PathBuf,
    seaco: Mutex<Option<Arc<SeacoSpeechRecognizer>>>,
    zipformer_model_directory: PathBuf,
    zipformer: Mutex<Option<Arc<ZipformerSpeechRecognizer>>>,
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
            live_preview: AtomicBool::new(false),
            whisper_model_directory: models_directory
                .join(WHISPER_MODEL_ID)
                .join(WHISPER_MODEL_REVISION),
            whisper: Mutex::new(None),
            qwen3_model_directory: models_directory
                .join(QWEN3_ASR_MODEL_ID)
                .join(QWEN3_ASR_MODEL_REVISION),
            qwen3: Mutex::new(None),
            seaco_model_directory: models_directory
                .join(SEACO_MODEL_ID)
                .join(SEACO_MODEL_REVISION),
            seaco: Mutex::new(None),
            zipformer_model_directory: models_directory
                .join(ZIPFORMER_MODEL_ID)
                .join(ZIPFORMER_MODEL_REVISION),
            zipformer: Mutex::new(None),
        }
    }

    pub fn prepare_whisper(&self) -> Result<(), SpeechRecognitionError> {
        ensure_self_test(self.whisper_recognizer()?.as_ref())
    }

    pub fn clear_whisper(&self) {
        if let Ok(mut recognizer) = self.whisper.lock() {
            recognizer.take();
        }
    }

    pub fn prepare_qwen3(&self) -> Result<(), SpeechRecognitionError> {
        ensure_self_test(self.qwen3_recognizer()?.as_ref())
    }

    pub fn clear_qwen3(&self) {
        if let Ok(mut recognizer) = self.qwen3.lock() {
            recognizer.take();
        }
    }

    pub fn shows_live_preview(&self) -> bool {
        self.live_preview.load(Ordering::Acquire)
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
        let qwen_mode = catalog
            .qwen_recognition_mode()
            .map_err(|error| SpeechRecognitionError::Protocol(error.to_string()))?;
        let whisper_mode = catalog
            .whisper_recognition_mode()
            .map_err(|error| SpeechRecognitionError::Protocol(error.to_string()))?;
        let seaco_active = catalog.seaco_is_active();
        let zipformer_active = catalog.zipformer_is_active();
        if !seaco_active
            && !zipformer_active
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
        } else if qwen3_active {
            self.qwen3_recognizer()?
                .start_with_mode(qwen_mode, hints, on_partial)?
        } else if whisper_active {
            self.whisper_recognizer()?
                .start_with_mode(whisper_mode, hints, on_partial)?
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
        self.live_preview.store(
            (qwen3_active && qwen_mode == template_app::QwenRecognitionMode::Streaming)
                || (whisper_active
                    && whisper_mode == template_app::WhisperRecognitionMode::Streaming),
            Ordering::Release,
        );
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
        additional_models::load_cached(
            &self.whisper,
            &self.whisper_model_directory,
            WhisperSpeechRecognizer::load,
        )
    }

    pub fn qwen3_recognizer(
        &self,
    ) -> Result<Arc<Qwen3AsrSpeechRecognizer>, SpeechRecognitionError> {
        additional_models::load_cached(
            &self.qwen3,
            &self.qwen3_model_directory,
            Qwen3AsrSpeechRecognizer::load,
        )
    }

    pub fn qwen3_process_memory(&self) -> Option<u64> {
        self.qwen3
            .try_lock()
            .ok()?
            .as_ref()?
            .resident_memory_bytes()
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
