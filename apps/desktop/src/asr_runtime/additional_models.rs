use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use template_app::{SpeechRecognitionError, StreamingSpeechRecognizer};
use template_infra::{MoonshineSpeechRecognizer, SeacoSpeechRecognizer, ZipformerSpeechRecognizer};

use super::{
    AsrSessionController, current_process_resident_memory_bytes, ensure_self_test,
    resident_memory_delta,
};

#[derive(Clone)]
pub(super) struct LoadedLocal {
    pub(super) recognizer: Arc<dyn StreamingSpeechRecognizer>,
    pub(super) resident_memory_bytes: Option<u64>,
}

fn load_cached<T: StreamingSpeechRecognizer + 'static>(
    cache: &Mutex<Option<LoadedLocal>>,
    directory: &Path,
    load: impl FnOnce(&Path) -> Result<T, SpeechRecognitionError>,
) -> Result<LoadedLocal, SpeechRecognitionError> {
    let mut cache = cache.lock().map_err(|_| {
        SpeechRecognitionError::Transport("local ASR cache lock was poisoned".to_owned())
    })?;
    if let Some(loaded) = cache.as_ref() {
        return Ok(loaded.clone());
    }
    let before = current_process_resident_memory_bytes();
    let recognizer = Arc::new(load(directory)?);
    let loaded = LoadedLocal {
        recognizer,
        resident_memory_bytes: resident_memory_delta(
            before,
            current_process_resident_memory_bytes(),
        ),
    };
    *cache = Some(loaded.clone());
    Ok(loaded)
}

impl AsrSessionController {
    pub fn seaco_recognizer(
        &self,
    ) -> Result<Arc<dyn StreamingSpeechRecognizer>, SpeechRecognitionError> {
        load_cached(
            &self.seaco,
            &self.seaco_model_directory,
            SeacoSpeechRecognizer::load,
        )
        .map(|loaded| loaded.recognizer)
    }

    pub fn zipformer_recognizer(
        &self,
    ) -> Result<Arc<dyn StreamingSpeechRecognizer>, SpeechRecognitionError> {
        load_cached(
            &self.zipformer,
            &self.zipformer_model_directory,
            ZipformerSpeechRecognizer::load,
        )
        .map(|loaded| loaded.recognizer)
    }

    pub fn moonshine_recognizer(
        &self,
    ) -> Result<Arc<dyn StreamingSpeechRecognizer>, SpeechRecognitionError> {
        load_cached(
            &self.moonshine,
            &self.moonshine_model_directory,
            MoonshineSpeechRecognizer::load,
        )
        .map(|loaded| loaded.recognizer)
    }

    pub fn prepare_seaco(&self) -> Result<Option<u64>, SpeechRecognitionError> {
        let loaded = load_cached(
            &self.seaco,
            &self.seaco_model_directory,
            SeacoSpeechRecognizer::load,
        )?;
        ensure_self_test(loaded.recognizer.as_ref())?;
        Ok(loaded.resident_memory_bytes)
    }

    pub fn prepare_zipformer(&self) -> Result<Option<u64>, SpeechRecognitionError> {
        let loaded = load_cached(
            &self.zipformer,
            &self.zipformer_model_directory,
            ZipformerSpeechRecognizer::load,
        )?;
        ensure_self_test(loaded.recognizer.as_ref())?;
        Ok(loaded.resident_memory_bytes)
    }

    pub fn prepare_moonshine(&self) -> Result<Option<u64>, SpeechRecognitionError> {
        let loaded = load_cached(
            &self.moonshine,
            &self.moonshine_model_directory,
            MoonshineSpeechRecognizer::load,
        )?;
        let (_, result) = super::self_test::run_english(loaded.recognizer.as_ref());
        result?;
        Ok(loaded.resident_memory_bytes)
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
    pub fn clear_moonshine(&self) {
        if let Ok(mut cache) = self.moonshine.lock() {
            cache.take();
        }
    }
}
