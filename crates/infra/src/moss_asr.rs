use crate::background_recognition::{BackgroundRecognition, Chunking};
#[cfg(test)]
use std::sync::Mutex;
use std::{path::Path, sync::Arc};
use template_app::{
    SpeechRecognitionError, SpeechRecognitionHints, StreamingRecognitionSession,
    StreamingSpeechRecognizer,
};

mod frontend;
#[cfg(test)]
mod native_tests;
mod prompt;
mod runtime;
#[cfg(test)]
mod tests;
mod transcript;

const SEGMENT_SAMPLES: usize = 30 * 16_000;

/// Experimental MOSS final transcription using pinned ONNX graphs on CPU.
/// Each 30-second segment has independent decoder state. Speaker/timestamp
/// annotations are removed from delivered text; vocabulary is session-owned.
pub struct MossSpeechRecognizer {
    decoder: Arc<dyn MossDecoder>,
}

impl MossSpeechRecognizer {
    pub fn load(directory: &Path) -> Result<Self, SpeechRecognitionError> {
        Ok(Self {
            decoder: Arc::new(runtime::OrtDecoder::load(directory)?),
        })
    }
}

impl StreamingSpeechRecognizer for MossSpeechRecognizer {
    fn start(
        &self,
        hints: SpeechRecognitionHints,
        on_partial: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Result<Box<dyn StreamingRecognitionSession>, SpeechRecognitionError> {
        let decoder = Arc::clone(&self.decoder);
        let hints = prompt::clean_hints(hints.terms());
        BackgroundRecognition::start(
            Chunking {
                window_samples: SEGMENT_SAMPLES,
                overlap_samples: 0,
            },
            move |samples| {
                if samples.iter().all(|sample| *sample == 0) {
                    return Ok(String::new());
                }
                decoder
                    .decode(samples, &hints)
                    .map(|raw| transcript::plain_text(&raw))
            },
            append_segment,
            on_partial,
        )
    }
}

/// Decodes a bounded segment with fresh KV state and never logs audio or hints.
trait MossDecoder: Send + Sync {
    fn decode(&self, samples: &[i16], hints: &str) -> Result<String, SpeechRecognitionError>;
}

fn append_segment(text: &mut String, plain: &str) {
    let plain = plain.trim();
    if text
        .chars()
        .last()
        .zip(plain.chars().next())
        .is_some_and(|(a, b)| a.is_ascii_alphanumeric() && b.is_ascii_alphanumeric())
    {
        text.push(' ');
    }
    text.push_str(plain);
}

fn protocol(message: &str) -> SpeechRecognitionError {
    SpeechRecognitionError::Protocol(format!("MOSS {message}"))
}

fn lock_error<T>(_error: T) -> SpeechRecognitionError {
    protocol("runtime lock was poisoned")
}
