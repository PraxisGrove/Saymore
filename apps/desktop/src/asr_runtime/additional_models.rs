use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use template_app::SpeechRecognitionError;
use template_infra::{SeacoSpeechRecognizer, ZipformerSpeechRecognizer};

use super::{AsrSessionController, ensure_self_test};

pub(super) fn load_cached<T>(
    cache: &Mutex<Option<Arc<T>>>,
    directory: &Path,
    load: impl FnOnce(&Path) -> Result<T, SpeechRecognitionError>,
) -> Result<Arc<T>, SpeechRecognitionError> {
    let mut cache = cache.lock().map_err(|_| {
        SpeechRecognitionError::Transport("local ASR cache lock was poisoned".to_owned())
    })?;
    if let Some(recognizer) = cache.as_ref() {
        return Ok(Arc::clone(recognizer));
    }
    let recognizer = Arc::new(load(directory)?);
    *cache = Some(Arc::clone(&recognizer));
    Ok(recognizer)
}

impl AsrSessionController {
    pub fn seaco_recognizer(&self) -> Result<Arc<SeacoSpeechRecognizer>, SpeechRecognitionError> {
        load_cached(
            &self.seaco,
            &self.seaco_model_directory,
            SeacoSpeechRecognizer::load,
        )
    }

    pub fn zipformer_recognizer(
        &self,
    ) -> Result<Arc<ZipformerSpeechRecognizer>, SpeechRecognitionError> {
        load_cached(
            &self.zipformer,
            &self.zipformer_model_directory,
            ZipformerSpeechRecognizer::load,
        )
    }

    pub fn prepare_seaco(&self) -> Result<(), SpeechRecognitionError> {
        ensure_self_test(self.seaco_recognizer()?.as_ref())
    }

    pub fn prepare_zipformer(&self) -> Result<(), SpeechRecognitionError> {
        ensure_self_test(self.zipformer_recognizer()?.as_ref())
    }

    pub fn clear_seaco(&self) {
        if let Ok(mut cache) = self.seaco.lock() {
            cache.take();
        }
    }
    pub fn clear_zipformer(&self) {
        if let Ok(mut cache) = self.zipformer.lock() {
            cache.take();
        }
    }
}
