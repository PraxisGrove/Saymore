use std::path::Path;

use sherpa_onnx::{OfflinePunctuation, OfflinePunctuationConfig};
use template_app::SpeechRecognitionError;

pub struct OfflinePunctuationRestorer {
    punctuation: OfflinePunctuation,
}

impl OfflinePunctuationRestorer {
    pub fn load(model_directory: &Path) -> Result<Self, SpeechRecognitionError> {
        let model_path = model_directory.join("model.int8.onnx");
        if !model_path.is_file() {
            return Err(SpeechRecognitionError::NotConfigured);
        }
        let model = model_path.to_str().ok_or_else(|| {
            SpeechRecognitionError::Protocol("punctuation model path is not Unicode".to_owned())
        })?;
        let mut config = OfflinePunctuationConfig::default();
        config.model.ct_transformer = Some(model.to_owned());
        let punctuation = OfflinePunctuation::create(&config).ok_or_else(|| {
            SpeechRecognitionError::Protocol(
                "failed to initialize the local punctuation model".to_owned(),
            )
        })?;
        Ok(Self { punctuation })
    }

    pub fn add_punctuation(&self, text: &str) -> Result<String, SpeechRecognitionError> {
        self.punctuation.add_punctuation(text).ok_or_else(|| {
            SpeechRecognitionError::Protocol("local punctuation inference failed".to_owned())
        })
    }
}
