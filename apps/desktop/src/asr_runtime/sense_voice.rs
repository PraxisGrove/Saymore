use std::sync::Arc;

use template_app::SpeechRecognitionError;
use template_infra::SenseVoiceSpeechRecognizer;

use super::{
    AsrSessionController, current_process_resident_memory_bytes, ensure_self_test,
    resident_memory_delta,
};

pub(super) struct LoadedSenseVoice {
    pub(super) recognizer: Arc<SenseVoiceSpeechRecognizer>,
    pub(super) resident_memory_bytes: Option<u64>,
}

impl AsrSessionController {
    pub fn prepare_sense_voice(&self) -> Result<Option<u64>, SpeechRecognitionError> {
        let loaded = self.sense_voice_runtime()?;
        ensure_self_test(loaded.recognizer.as_ref())?;
        Ok(loaded.resident_memory_bytes)
    }

    pub fn clear_sense_voice(&self) {
        if let Ok(mut recognizer) = self.sense_voice.lock() {
            recognizer.take();
        }
    }

    pub fn sense_voice_recognizer(
        &self,
    ) -> Result<Arc<SenseVoiceSpeechRecognizer>, SpeechRecognitionError> {
        self.sense_voice_runtime().map(|loaded| loaded.recognizer)
    }

    fn sense_voice_runtime(&self) -> Result<LoadedSenseVoice, SpeechRecognitionError> {
        let mut recognizer = self.sense_voice.lock().map_err(|_| {
            SpeechRecognitionError::Transport("SenseVoice cache lock was poisoned".to_owned())
        })?;
        if let Some(loaded) = recognizer.as_ref() {
            return Ok(loaded.clone());
        }
        let resident_before = current_process_resident_memory_bytes();
        let loaded = Arc::new(SenseVoiceSpeechRecognizer::load(
            &self.sense_voice_model_directory,
        )?);
        let resident_memory_bytes =
            resident_memory_delta(resident_before, current_process_resident_memory_bytes());
        let loaded = LoadedSenseVoice {
            recognizer: loaded,
            resident_memory_bytes,
        };
        *recognizer = Some(loaded.clone());
        Ok(loaded)
    }
}

impl Clone for LoadedSenseVoice {
    fn clone(&self) -> Self {
        Self {
            recognizer: Arc::clone(&self.recognizer),
            resident_memory_bytes: self.resident_memory_bytes,
        }
    }
}
