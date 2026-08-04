use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

#[cfg(target_os = "macos")]
use mach2::{
    kern_return::KERN_SUCCESS,
    task::task_info,
    task_info::{MACH_TASK_BASIC_INFO, MACH_TASK_BASIC_INFO_COUNT, mach_task_basic_info},
    traps::mach_task_self,
};
use template_app::{
    DictationSessionId, DictionaryStore, OwnedRecognition, ParaformerPunctuationMode, PcmRecording,
    RestoredRecordingTranscriber, SpeechRecognitionError, SpeechRecognitionHints,
    StreamingRecognitionSession, StreamingSpeechRecognizer,
};
#[cfg(target_os = "macos")]
use template_infra::MacOsSpeechRecognizer;
use template_infra::{
    JsonSettingsStore, OfflinePunctuationRestorer, OpenAiCompatibleSpeechRecognizer,
    PARAFORMER_MODEL_ID, PARAFORMER_MODEL_REVISION, PUNCTUATION_MODEL_ID,
    PUNCTUATION_MODEL_REVISION, ParaformerSpeechRecognizer, QWEN3_ASR_MODEL_ID,
    QWEN3_ASR_MODEL_REVISION, Qwen3AsrSpeechRecognizer, SENSE_VOICE_MODEL_ID,
    SENSE_VOICE_MODEL_REVISION, VolcengineSpeechRecognizer, WHISPER_MODEL_ID,
    WHISPER_MODEL_REVISION, WhisperSpeechRecognizer,
};

mod punctuation;
mod sense_voice;

use punctuation::PunctuatedRecognitionSession;
use sense_voice::LoadedSenseVoice;

pub struct AsrSessionController {
    settings: Arc<JsonSettingsStore>,
    dictionary: Arc<dyn DictionaryStore>,
    active: Mutex<Option<OwnedRecognition>>,
    paraformer_model_directory: PathBuf,
    paraformer: Mutex<Option<LoadedParaformer>>,
    punctuation_model_directory: PathBuf,
    punctuation: Mutex<Option<LoadedPunctuation>>,
    whisper_model_directory: PathBuf,
    whisper: Mutex<Option<LoadedWhisper>>,
    qwen3_model_directory: PathBuf,
    qwen3: Mutex<Option<LoadedQwen3>>,
    sense_voice_model_directory: PathBuf,
    sense_voice: Mutex<Option<LoadedSenseVoice>>,
}

struct LoadedParaformer {
    recognizer: Arc<ParaformerSpeechRecognizer>,
    resident_memory_bytes: Option<u64>,
}

struct LoadedPunctuation {
    restorer: Arc<OfflinePunctuationRestorer>,
    resident_memory_bytes: Option<u64>,
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
            paraformer_model_directory: models_directory
                .join(PARAFORMER_MODEL_ID)
                .join(PARAFORMER_MODEL_REVISION),
            paraformer: Mutex::new(None),
            punctuation_model_directory: models_directory
                .join(PUNCTUATION_MODEL_ID)
                .join(PUNCTUATION_MODEL_REVISION),
            punctuation: Mutex::new(None),
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
        }
    }

    pub fn prepare_paraformer(&self) -> Result<Option<u64>, SpeechRecognitionError> {
        self.paraformer_runtime()
            .map(|loaded| loaded.resident_memory_bytes)
    }

    pub fn clear_paraformer(&self) {
        if let Ok(mut recognizer) = self.paraformer.lock() {
            recognizer.take();
        }
    }

    pub fn prepare_punctuation(&self) -> Result<Option<u64>, SpeechRecognitionError> {
        self.punctuation_runtime()
            .map(|loaded| loaded.resident_memory_bytes)
    }

    pub fn clear_punctuation(&self) {
        if let Ok(mut punctuation) = self.punctuation.lock() {
            punctuation.take();
        }
    }

    pub fn restore_punctuation(&self, raw: String) -> String {
        if raw.trim().is_empty() {
            return raw;
        }
        match self
            .punctuation_runtime()
            .map(|punctuation| punctuation::restore_or_raw(&punctuation.restorer, raw.clone()))
        {
            Ok(punctuated) => punctuated,
            Err(error) => {
                tracing::warn!(
                    target: "saymore::diagnostics",
                    event = "punctuation.inference_failed",
                    reason = %error
                );
                raw
            }
        }
    }

    pub fn prepare_whisper(&self) -> Result<Option<u64>, SpeechRecognitionError> {
        self.whisper_runtime()
            .map(|loaded| loaded.resident_memory_bytes)
    }

    pub fn clear_whisper(&self) {
        if let Ok(mut recognizer) = self.whisper.lock() {
            recognizer.take();
        }
    }

    pub fn prepare_qwen3(&self) -> Result<Option<u64>, SpeechRecognitionError> {
        self.qwen3_runtime()
            .map(|loaded| loaded.resident_memory_bytes)
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
        let paraformer_active = catalog.paraformer_is_active();
        let whisper_active = catalog.whisper_is_active();
        let qwen3_active = catalog.qwen3_asr_is_active();
        let sense_voice_active = catalog.sense_voice_is_active();
        if !sense_voice_active
            && !qwen3_active
            && !whisper_active
            && !paraformer_active
            && !macos_speech_active
            && !settings.asr.volcengine.enabled
            && !settings.asr.openai_compatible.enabled
        {
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
        let session = if sense_voice_active {
            self.sense_voice_recognizer()?.start(hints, on_partial)?
        } else if qwen3_active {
            self.qwen3_recognizer()?.start(hints, on_partial)?
        } else if whisper_active {
            self.whisper_recognizer()?.start(hints, on_partial)?
        } else if paraformer_active {
            self.start_paraformer_session(catalog.paraformer_punctuation_mode(), hints, on_partial)?
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

    pub fn paraformer_recognizer(
        &self,
    ) -> Result<Arc<ParaformerSpeechRecognizer>, SpeechRecognitionError> {
        self.paraformer_runtime().map(|loaded| loaded.recognizer)
    }

    fn start_paraformer_session(
        &self,
        punctuation_mode: ParaformerPunctuationMode,
        hints: SpeechRecognitionHints,
        on_partial: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Result<Box<dyn StreamingRecognitionSession>, SpeechRecognitionError> {
        let session = self.paraformer_recognizer()?.start(hints, on_partial)?;
        if punctuation_mode != ParaformerPunctuationMode::Local {
            return Ok(session);
        }
        match self.punctuation_runtime() {
            Ok(punctuation) => Ok(Box::new(PunctuatedRecognitionSession {
                inner: session,
                punctuation: punctuation.restorer,
            })),
            Err(error) => {
                tracing::warn!(
                    target: "saymore::diagnostics",
                    event = "punctuation.runtime_unavailable",
                    reason = %error
                );
                Ok(session)
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

    fn paraformer_runtime(&self) -> Result<LoadedParaformer, SpeechRecognitionError> {
        let mut recognizer = self.paraformer.lock().map_err(|_| {
            SpeechRecognitionError::Transport("Paraformer cache lock was poisoned".to_owned())
        })?;
        if let Some(loaded) = recognizer.as_ref() {
            return Ok(loaded.clone());
        }
        let resident_before = current_process_resident_memory_bytes();
        let loaded = Arc::new(ParaformerSpeechRecognizer::load(
            &self.paraformer_model_directory,
        )?);
        let resident_memory_bytes =
            resident_memory_delta(resident_before, current_process_resident_memory_bytes());
        let loaded = LoadedParaformer {
            recognizer: loaded,
            resident_memory_bytes,
        };
        *recognizer = Some(loaded.clone());
        Ok(loaded)
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

    fn punctuation_runtime(&self) -> Result<LoadedPunctuation, SpeechRecognitionError> {
        let mut punctuation = self.punctuation.lock().map_err(|_| {
            SpeechRecognitionError::Transport("punctuation cache lock was poisoned".to_owned())
        })?;
        if let Some(loaded) = punctuation.as_ref() {
            return Ok(loaded.clone());
        }
        let resident_before = current_process_resident_memory_bytes();
        let restorer = Arc::new(OfflinePunctuationRestorer::load(
            &self.punctuation_model_directory,
        )?);
        let resident_memory_bytes =
            resident_memory_delta(resident_before, current_process_resident_memory_bytes());
        let loaded = LoadedPunctuation {
            restorer,
            resident_memory_bytes,
        };
        *punctuation = Some(loaded.clone());
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

impl Clone for LoadedParaformer {
    fn clone(&self) -> Self {
        Self {
            recognizer: Arc::clone(&self.recognizer),
            resident_memory_bytes: self.resident_memory_bytes,
        }
    }
}

impl Clone for LoadedPunctuation {
    fn clone(&self) -> Self {
        Self {
            restorer: Arc::clone(&self.restorer),
            resident_memory_bytes: self.resident_memory_bytes,
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

#[cfg(target_os = "macos")]
pub(crate) fn current_process_resident_memory_bytes() -> Option<u64> {
    let mut info = mach_task_basic_info::default();
    let mut count = MACH_TASK_BASIC_INFO_COUNT;
    // SAFETY: `info` is a writable Mach task-info buffer of the advertised size.
    let status = unsafe {
        task_info(
            mach_task_self(),
            MACH_TASK_BASIC_INFO,
            std::ptr::addr_of_mut!(info).cast(),
            &mut count,
        )
    };
    if status != KERN_SUCCESS {
        return None;
    }
    // `mach2` models this ABI structure as packed to four-byte alignment.
    Some(unsafe { std::ptr::addr_of!(info.resident_size).read_unaligned() })
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn current_process_resident_memory_bytes() -> Option<u64> {
    None
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
