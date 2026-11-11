use std::{
    fs,
    path::Path,
    sync::{Arc, Mutex},
};

use sherpa_onnx::{OfflineMoonshineModelConfig, OfflineRecognizer, OfflineRecognizerConfig};
use template_app::{
    SpeechRecognitionError, SpeechRecognitionHints, StreamingRecognitionSession,
    StreamingSpeechRecognizer,
};

#[cfg(test)]
#[allow(clippy::panic_in_result_fn)]
mod tests;

const SAMPLE_RATE: i32 = 16_000;
const MAX_SEGMENT_SAMPLES: usize = 30 * SAMPLE_RATE as usize;
const MODEL_FILES: [(&str, u64); 5] = [
    ("preprocess.onnx", 6_800_738),
    ("encode.int8.onnx", 18_249_187),
    ("uncached_decode.int8.onnx", 53_216_096),
    ("cached_decode.int8.onnx", 45_264_830),
    ("tokens.txt", 436_688),
];

/// Loads the pinned English-only Moonshine tiny INT8 model for final decoding.
/// This model does not support recognition-time vocabulary hints or partials.
pub struct MoonshineSpeechRecognizer {
    decoder: Arc<dyn MoonshineDecoder>,
}

impl MoonshineSpeechRecognizer {
    pub fn load(model_directory: &Path) -> Result<Self, SpeechRecognitionError> {
        validate_files(model_directory)?;
        let mut config = OfflineRecognizerConfig::default();
        config.model_config.moonshine = OfflineMoonshineModelConfig {
            preprocessor: Some(model_path(model_directory, "preprocess.onnx")?),
            encoder: Some(model_path(model_directory, "encode.int8.onnx")?),
            uncached_decoder: Some(model_path(model_directory, "uncached_decode.int8.onnx")?),
            cached_decoder: Some(model_path(model_directory, "cached_decode.int8.onnx")?),
            ..Default::default()
        };
        config.model_config.tokens = Some(model_path(model_directory, "tokens.txt")?);
        config.model_config.num_threads = 4;
        config.model_config.provider = Some("cpu".to_owned());
        config.decoding_method = Some("greedy_search".to_owned());
        let recognizer = OfflineRecognizer::create(&config).ok_or_else(|| {
            SpeechRecognitionError::Protocol(
                "the Moonshine runtime could not load its model".to_owned(),
            )
        })?;
        Ok(Self {
            decoder: Arc::new(SherpaMoonshineDecoder(Mutex::new(recognizer))),
        })
    }
}

impl StreamingSpeechRecognizer for MoonshineSpeechRecognizer {
    fn start(
        &self,
        _hints: SpeechRecognitionHints,
        _on_partial: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Result<Box<dyn StreamingRecognitionSession>, SpeechRecognitionError> {
        Ok(Box::new(MoonshineSession {
            decoder: Arc::clone(&self.decoder),
            samples: Mutex::new(Vec::new()),
        }))
    }
}

fn validate_files(directory: &Path) -> Result<(), SpeechRecognitionError> {
    if !directory.is_dir() {
        return Err(SpeechRecognitionError::NotConfigured);
    }
    for (name, bytes) in MODEL_FILES {
        let metadata = match fs::metadata(directory.join(name)) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(SpeechRecognitionError::NotConfigured);
            }
            Err(_) => {
                return Err(SpeechRecognitionError::Transport(
                    "could not inspect Moonshine model files".to_owned(),
                ));
            }
        };
        if !metadata.is_file() || metadata.len() != bytes {
            return Err(SpeechRecognitionError::Protocol(
                "model files do not match the pinned Moonshine artifact".to_owned(),
            ));
        }
    }
    Ok(())
}

fn model_path(directory: &Path, name: &str) -> Result<String, SpeechRecognitionError> {
    directory
        .join(name)
        .to_str()
        .filter(|text| !text.contains('\0'))
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            SpeechRecognitionError::Protocol(
                "Moonshine requires Unicode model paths without NUL".to_owned(),
            )
        })
}

/// Decodes one ordered PCM segment without retaining audio from earlier sessions.
trait MoonshineDecoder: Send + Sync {
    fn decode(&self, samples: &[i16]) -> Result<String, SpeechRecognitionError>;
}

struct SherpaMoonshineDecoder(Mutex<OfflineRecognizer>);

impl MoonshineDecoder for SherpaMoonshineDecoder {
    fn decode(&self, samples: &[i16]) -> Result<String, SpeechRecognitionError> {
        let recognizer = self.0.lock().map_err(lock_error)?;
        // Generic hotword streams are unsupported for Moonshine in sherpa 1.13.4.
        let stream = recognizer.create_stream();
        let waveform = samples
            .iter()
            .map(|sample| f32::from(*sample) / 32_768.0)
            .collect::<Vec<_>>();
        stream.accept_waveform(SAMPLE_RATE, &waveform);
        recognizer.decode(&stream);
        stream
            .get_result()
            .map(|result| result.text)
            .ok_or_else(|| {
                SpeechRecognitionError::Protocol(
                    "Moonshine returned no recognition result".to_owned(),
                )
            })
    }
}

struct MoonshineSession {
    decoder: Arc<dyn MoonshineDecoder>,
    samples: Mutex<Vec<i16>>,
}

impl StreamingRecognitionSession for MoonshineSession {
    fn push_audio(&self, samples: Vec<i16>) -> Result<(), SpeechRecognitionError> {
        self.samples.lock().map_err(lock_error)?.extend(samples);
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<String, SpeechRecognitionError> {
        let samples = self.samples.into_inner().map_err(lock_error)?;
        if samples.is_empty() {
            return Err(SpeechRecognitionError::Protocol(
                "Moonshine received no audio".to_owned(),
            ));
        }
        // Bound each encoder invocation to a short utterance; keep all PCM in order.
        let mut transcript = String::new();
        for segment in samples.chunks(MAX_SEGMENT_SAMPLES) {
            let decoded = self.decoder.decode(segment)?;
            let text = decoded.trim();
            if !text.is_empty() {
                if !transcript.is_empty() {
                    transcript.push(' ');
                }
                transcript.push_str(text);
            }
        }
        if transcript.is_empty() {
            Err(SpeechRecognitionError::Protocol(
                "Moonshine returned an empty final transcript".to_owned(),
            ))
        } else {
            Ok(transcript)
        }
    }

    fn cancel(self: Box<Self>) {}
}

fn lock_error(_error: impl std::fmt::Display) -> SpeechRecognitionError {
    SpeechRecognitionError::Transport("Moonshine runtime lock failed".to_owned())
}
