use std::{
    fs,
    path::{Path, PathBuf},
};

use template_app::SpeechRecognitionError;

use super::protocol;

pub(super) struct ModelFiles {
    pub model: PathBuf,
    pub embedding: PathBuf,
    pub tokens: PathBuf,
    pub segmentation: PathBuf,
    pub cmvn: PathBuf,
}

impl ModelFiles {
    pub fn validate(directory: &Path) -> Result<Self, SpeechRecognitionError> {
        Ok(Self {
            model: file(directory, "model.onnx", 957_692_158..=957_692_158)?,
            embedding: file(directory, "model_eb.onnx", 34_026_072..=34_026_072)?,
            tokens: file(directory, "tokens.json", 1..=1_000_000)?,
            segmentation: file(directory, "seg_dict", 1..=32_000_000)?,
            cmvn: file(directory, "am.mvn", 1..=128_000)?,
        })
    }
}

fn file(
    directory: &Path,
    name: &str,
    bytes: std::ops::RangeInclusive<u64>,
) -> Result<PathBuf, SpeechRecognitionError> {
    let path = directory.join(name);
    match fs::metadata(&path) {
        Ok(metadata) if metadata.is_file() && bytes.contains(&metadata.len()) => Ok(path),
        Ok(_) => Err(protocol(
            "model files do not match the supported FP32 export",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Err(SpeechRecognitionError::NotConfigured)
        }
        Err(_) => Err(SpeechRecognitionError::Transport(
            "could not inspect SeACo model files".to_owned(),
        )),
    }
}
