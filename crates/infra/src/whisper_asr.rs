use std::{
    fs,
    path::Path,
    sync::{Arc, Mutex, Once},
};

use template_app::{
    SpeechRecognitionError, SpeechRecognitionHints, StreamingRecognitionSession,
    StreamingSpeechRecognizer,
};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::model_installer::{WHISPER_MODEL_BYTES, WHISPER_MODEL_FILE};

mod hints;
#[cfg(test)]
mod tests;

const SAMPLE_RATE: i32 = 16_000;
const MAX_SEGMENT_SAMPLES: usize = 29 * SAMPLE_RATE as usize;
static LOGGING: Once = Once::new();

/// Loads Whisper large-v3-turbo with whisper.cpp and per-dictation vocabulary context.
pub struct WhisperSpeechRecognizer {
    decoder: Arc<dyn WhisperDecoder>,
}

impl WhisperSpeechRecognizer {
    pub fn load(model_directory: &Path) -> Result<Self, SpeechRecognitionError> {
        let model = model_directory.join(WHISPER_MODEL_FILE);
        match fs::metadata(&model) {
            Ok(metadata) if metadata.is_file() && metadata.len() == WHISPER_MODEL_BYTES => {}
            Ok(_) => {
                return Err(SpeechRecognitionError::Protocol(
                    "the Whisper model does not match the pinned artifact".to_owned(),
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(SpeechRecognitionError::NotConfigured);
            }
            Err(_) => {
                return Err(SpeechRecognitionError::Transport(
                    "could not inspect the Whisper model file".to_owned(),
                ));
            }
        }
        // Native logs may contain paths and vocabulary. Without a logging backend,
        // whisper-rs' hooks discard them rather than exposing user content.
        LOGGING.call_once(whisper_rs::install_logging_hooks);
        let mut parameters = WhisperContextParameters::default();
        parameters.use_gpu(false);
        let context = WhisperContext::new_with_params(&model, parameters)
            .map_err(|error| runtime_error("load the model", error))?;
        Ok(Self {
            decoder: Arc::new(CppWhisperDecoder {
                context: Mutex::new(context),
            }),
        })
    }

    #[cfg(test)]
    fn with_decoder(decoder: Arc<dyn WhisperDecoder>) -> Self {
        Self { decoder }
    }
}

impl StreamingSpeechRecognizer for WhisperSpeechRecognizer {
    fn start(
        &self,
        hints: SpeechRecognitionHints,
        _on_partial: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Result<Box<dyn StreamingRecognitionSession>, SpeechRecognitionError> {
        Ok(Box::new(WhisperSession {
            decoder: Arc::clone(&self.decoder),
            prompt: hints::vocabulary_prompt(hints.terms()),
            samples: Mutex::new(Vec::new()),
        }))
    }
}

/// Decodes one audio segment using only that dictation's bounded vocabulary snapshot.
/// Implementations must isolate decoder state between calls and never log the prompt.
trait WhisperDecoder: Send + Sync {
    fn decode(&self, samples: &[i16], prompt: &str) -> Result<String, SpeechRecognitionError>;
}

struct CppWhisperDecoder {
    context: Mutex<WhisperContext>,
}

impl WhisperDecoder for CppWhisperDecoder {
    fn decode(&self, samples: &[i16], prompt: &str) -> Result<String, SpeechRecognitionError> {
        let context = self.context.lock().map_err(lock_error)?;
        // Whisper's byte-level tokenizer cannot produce more tokens than input
        // bytes. Allocate that upper bound rather than a smaller token budget.
        let tokens = context
            .tokenize(prompt, prompt.len() + 1)
            .map_err(|error| runtime_error("tokenize vocabulary", error))?;
        let mut state = context
            .create_state()
            .map_err(|error| runtime_error("create decoder state", error))?;
        let mut parameters = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        parameters.set_n_threads(4);
        parameters.set_translate(false);
        parameters.set_language(None);
        parameters.set_no_context(true);
        parameters.set_print_special(false);
        parameters.set_print_progress(false);
        parameters.set_print_realtime(false);
        parameters.set_print_timestamps(false);
        // This is whisper.cpp's initial-prompt token path. The borrowed tokens
        // live through full(), avoiding the binding's allocating string setter.
        parameters.set_tokens(&tokens);
        state
            .full(parameters, &pcm_f32(samples))
            .map_err(|error| runtime_error("decode audio", error))?;
        let mut transcript = String::new();
        for segment in state.as_iter() {
            let text = segment
                .to_str()
                .map_err(|error| runtime_error("read decoded text", error))?;
            transcript.push_str(text);
        }
        Ok(transcript)
    }
}

struct WhisperSession {
    decoder: Arc<dyn WhisperDecoder>,
    prompt: String,
    samples: Mutex<Vec<i16>>,
}

impl StreamingRecognitionSession for WhisperSession {
    fn push_audio(&self, samples: Vec<i16>) -> Result<(), SpeechRecognitionError> {
        self.samples.lock().map_err(lock_error)?.extend(samples);
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<String, SpeechRecognitionError> {
        let samples = self.samples.into_inner().map_err(lock_error)?;
        if samples.is_empty() {
            return Err(SpeechRecognitionError::Protocol(
                "Whisper received no audio".to_owned(),
            ));
        }
        let mut transcript = String::new();
        for segment in samples.chunks(MAX_SEGMENT_SAMPLES) {
            append_segment(
                &mut transcript,
                &self.decoder.decode(segment, &self.prompt)?,
            );
        }
        if transcript.is_empty() {
            Err(SpeechRecognitionError::Protocol(
                "Whisper returned an empty final transcript".to_owned(),
            ))
        } else {
            Ok(transcript)
        }
    }

    fn cancel(self: Box<Self>) {}
}

fn append_segment(transcript: &mut String, segment: &str) {
    let segment = segment.trim();
    if segment.is_empty() {
        return;
    }
    let needs_space = transcript
        .chars()
        .last()
        .zip(segment.chars().next())
        .is_some_and(|(left, right)| left.is_ascii_alphanumeric() && right.is_ascii_alphanumeric());
    if needs_space {
        transcript.push(' ');
    }
    transcript.push_str(segment);
}

fn pcm_f32(samples: &[i16]) -> Vec<f32> {
    samples
        .iter()
        .map(|sample| f32::from(*sample) / 32_768.0)
        .collect()
}

fn runtime_error(operation: &str, error: impl std::fmt::Display) -> SpeechRecognitionError {
    SpeechRecognitionError::Protocol(format!("Whisper could not {operation}: {error}"))
}

fn lock_error(_error: impl std::fmt::Display) -> SpeechRecognitionError {
    SpeechRecognitionError::Transport("Whisper runtime lock was poisoned".to_owned())
}
