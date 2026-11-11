use std::{
    collections::BTreeSet,
    fs,
    path::Path,
    sync::{Arc, Mutex},
};

use sherpa_onnx::{OnlineRecognizer, OnlineRecognizerConfig, OnlineStream};
use template_app::{
    SpeechRecognitionError, SpeechRecognitionHints, StreamingRecognitionSession,
    StreamingSpeechRecognizer,
};

mod hotwords;
#[cfg(test)]
#[allow(clippy::panic_in_result_fn)]
mod tests;

const SAMPLE_RATE: i32 = 16_000;
const TAIL_PADDING_SAMPLES: usize = 8_000;
const MODEL_FILES: [(&str, u64); 5] = [
    ("encoder-epoch-99-avg-1.int8.onnx", 181_895_032),
    // Keep these FP32: the all-INT8 variant repeats syllables on standard-zh.pcm.
    ("decoder-epoch-99-avg-1.onnx", 13_876_452),
    ("joiner-epoch-99-avg-1.onnx", 12_833_618),
    ("tokens.txt", 56_317),
    ("bpe.vocab", 12_564),
];

/// Loads the pinned bilingual streaming Zipformer with an INT8 encoder.
/// Each session owns its transducer stream and a bounded vocabulary snapshot.
/// Vocabulary biases decoding; it does not guarantee a particular spelling.
pub struct ZipformerSpeechRecognizer {
    decoder: Arc<dyn ZipformerDecoder>,
    vocabulary: BTreeSet<char>,
}

impl ZipformerSpeechRecognizer {
    pub fn load(model_directory: &Path) -> Result<Self, SpeechRecognitionError> {
        validate_files(model_directory)?;
        let tokens = fs::read_to_string(model_directory.join("tokens.txt")).map_err(|_| {
            SpeechRecognitionError::Transport("could not read Zipformer vocabulary".to_owned())
        })?;
        let vocabulary = hotwords::supported_characters(&tokens);
        let mut config = OnlineRecognizerConfig::default();
        config.model_config.transducer.encoder =
            Some(model_path(model_directory, MODEL_FILES[0].0)?);
        config.model_config.transducer.decoder =
            Some(model_path(model_directory, MODEL_FILES[1].0)?);
        config.model_config.transducer.joiner =
            Some(model_path(model_directory, MODEL_FILES[2].0)?);
        config.model_config.tokens = Some(model_path(model_directory, "tokens.txt")?);
        config.model_config.bpe_vocab = Some(model_path(model_directory, "bpe.vocab")?);
        config.model_config.modeling_unit = Some("cjkchar+bpe".to_owned());
        config.model_config.num_threads = 4;
        config.model_config.provider = Some("cpu".to_owned());
        config.decoding_method = Some("modified_beam_search".to_owned());
        config.max_active_paths = 4;
        config.hotwords_score = 1.5;
        // Do not reset on endpoint detection: one dictation must retain all speech.
        config.enable_endpoint = false;
        let recognizer = OnlineRecognizer::create(&config).ok_or_else(|| {
            SpeechRecognitionError::Protocol(
                "the Zipformer runtime could not load its model".to_owned(),
            )
        })?;
        Ok(Self {
            decoder: Arc::new(SherpaZipformerDecoder(Arc::new(recognizer))),
            vocabulary,
        })
    }
}

impl StreamingSpeechRecognizer for ZipformerSpeechRecognizer {
    fn start(
        &self,
        hints: SpeechRecognitionHints,
        on_partial: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Result<Box<dyn StreamingRecognitionSession>, SpeechRecognitionError> {
        let hotwords = hotwords::from_hints(&hints, &self.vocabulary);
        Ok(Box::new(ZipformerSession {
            state: Mutex::new(SessionState {
                stream: self.decoder.create_stream(&hotwords),
                last_partial: String::new(),
                received_audio: false,
            }),
            on_partial,
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
                    "could not inspect Zipformer model files".to_owned(),
                ));
            }
        };
        if !metadata.is_file() || metadata.len() != bytes {
            return Err(SpeechRecognitionError::Protocol(
                "model files do not match the pinned Zipformer artifact".to_owned(),
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
                "Zipformer requires Unicode model paths without NUL".to_owned(),
            )
        })
}

/// Creates isolated streams; implementations must not share session hotwords.
trait ZipformerDecoder: Send + Sync {
    fn create_stream(&self, hotwords: &str) -> Box<dyn ZipformerStream>;
}

/// Owns one stream; ready frames must be drained and finalization is explicit.
trait ZipformerStream: Send {
    fn accept_audio(&mut self, samples: &[f32]);
    fn decode_ready(&mut self);
    fn text(&mut self) -> Option<String>;
    fn finish_input(&mut self);
}

struct SherpaZipformerDecoder(Arc<OnlineRecognizer>);

impl ZipformerDecoder for SherpaZipformerDecoder {
    fn create_stream(&self, hotwords: &str) -> Box<dyn ZipformerStream> {
        let stream = if hotwords.is_empty() {
            self.0.create_stream()
        } else {
            // Supported by this transducer with modified_beam_search and cjkchar+bpe.
            self.0.create_stream_with_hotwords(hotwords)
        };
        Box::new(SherpaZipformerStream {
            recognizer: Arc::clone(&self.0),
            stream,
        })
    }
}

struct SherpaZipformerStream {
    recognizer: Arc<OnlineRecognizer>,
    stream: OnlineStream,
}

impl ZipformerStream for SherpaZipformerStream {
    fn accept_audio(&mut self, samples: &[f32]) {
        self.stream.accept_waveform(SAMPLE_RATE, samples);
    }

    fn decode_ready(&mut self) {
        while self.recognizer.is_ready(&self.stream) {
            self.recognizer.decode(&self.stream);
        }
    }

    fn text(&mut self) -> Option<String> {
        self.recognizer
            .get_result(&self.stream)
            .map(|result| result.text)
    }

    fn finish_input(&mut self) {
        self.stream.input_finished();
    }
}

struct SessionState {
    stream: Box<dyn ZipformerStream>,
    last_partial: String,
    received_audio: bool,
}

struct ZipformerSession {
    state: Mutex<SessionState>,
    on_partial: Arc<dyn Fn(String) + Send + Sync>,
}

impl StreamingRecognitionSession for ZipformerSession {
    fn push_audio(&self, samples: Vec<i16>) -> Result<(), SpeechRecognitionError> {
        if samples.is_empty() {
            return Ok(());
        }
        let partial = {
            let mut state = self.state.lock().map_err(lock_error)?;
            state.received_audio = true;
            for chunk in samples.chunks(1_600) {
                let waveform = chunk
                    .iter()
                    .map(|sample| f32::from(*sample) / 32_768.0)
                    .collect::<Vec<_>>();
                state.stream.accept_audio(&waveform);
                state.stream.decode_ready();
            }
            let text = state.stream.text().unwrap_or_default().trim().to_owned();
            if text.is_empty() || text == state.last_partial {
                None
            } else {
                state.last_partial.clone_from(&text);
                Some(text)
            }
        };
        if let Some(text) = partial {
            (self.on_partial)(text);
        }
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<String, SpeechRecognitionError> {
        let mut state = self.state.into_inner().map_err(lock_error)?;
        if !state.received_audio {
            return Err(SpeechRecognitionError::Protocol(
                "Zipformer received no audio".to_owned(),
            ));
        }
        state.stream.accept_audio(&[0.0; TAIL_PADDING_SAMPLES]);
        state.stream.finish_input();
        state.stream.decode_ready();
        let text = state.stream.text().unwrap_or_default().trim().to_owned();
        if text.is_empty() {
            Err(SpeechRecognitionError::Protocol(
                "Zipformer returned an empty final transcript".to_owned(),
            ))
        } else {
            Ok(text)
        }
    }

    fn cancel(self: Box<Self>) {}
}

fn lock_error(_error: impl std::fmt::Display) -> SpeechRecognitionError {
    SpeechRecognitionError::Transport("Zipformer session lock failed".to_owned())
}
