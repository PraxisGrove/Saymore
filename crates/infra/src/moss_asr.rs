use std::{
    path::Path,
    sync::{Arc, Mutex},
};
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
        _on_partial: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Result<Box<dyn StreamingRecognitionSession>, SpeechRecognitionError> {
        Ok(Box::new(MossSession {
            decoder: Arc::clone(&self.decoder),
            hints: prompt::clean_hints(hints.terms()),
            samples: Mutex::new(Vec::new()),
        }))
    }
}

/// Decodes a bounded segment with fresh KV state and never logs audio or hints.
trait MossDecoder: Send + Sync {
    fn decode(&self, samples: &[i16], hints: &str) -> Result<String, SpeechRecognitionError>;
}

struct MossSession {
    decoder: Arc<dyn MossDecoder>,
    hints: String,
    samples: Mutex<Vec<i16>>,
}

impl StreamingRecognitionSession for MossSession {
    fn push_audio(&self, samples: Vec<i16>) -> Result<(), SpeechRecognitionError> {
        let mut buffered = self.samples.lock().map_err(lock_error)?;
        if buffered.len().saturating_add(samples.len()) > 30 * 60 * 16_000 {
            return Err(protocol("recording exceeds the 30-minute limit"));
        }
        buffered.extend(samples);
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<String, SpeechRecognitionError> {
        let samples = self.samples.into_inner().map_err(lock_error)?;
        if samples.is_empty() {
            return Err(protocol("received no audio"));
        }
        let mut text = String::new();
        for segment in samples.chunks(SEGMENT_SAMPLES) {
            if segment.iter().all(|sample| *sample == 0) {
                continue;
            }
            let decoded = self.decoder.decode(segment, &self.hints)?;
            let plain = transcript::plain_text(&decoded);
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
        if text.is_empty() {
            Err(protocol("returned no speech"))
        } else {
            Ok(text)
        }
    }

    fn cancel(self: Box<Self>) {}
}

fn protocol(message: &str) -> SpeechRecognitionError {
    SpeechRecognitionError::Protocol(format!("MOSS {message}"))
}

fn lock_error<T>(_error: T) -> SpeechRecognitionError {
    protocol("runtime lock was poisoned")
}
