use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use template_app::{
    SpeechRecognitionError, SpeechRecognitionHints, StreamingRecognitionSession,
    StreamingSpeechRecognizer,
};

mod frontend;
mod model;
mod runtime;
#[cfg(test)]
#[allow(clippy::panic_in_result_fn)]
mod tests;
mod vocabulary;

use vocabulary::{Hotwords, Vocabulary};

const SAMPLE_RATE: usize = 16_000;
const MAX_SEGMENT_SAMPLES: usize = 30 * SAMPLE_RATE;

/// CPU SeACo-Paraformer FP32 with recognition-time vocabulary embeddings.
///
/// Loads the exact exported `model.onnx`, `model_eb.onnx`, `tokens.json`,
/// `seg_dict` and `am.mvn`. The installer owns artifact hash verification.
/// Audio is 16 kHz mono i16 PCM. Final decoding uses independent 30-second
/// segments; this offline graph does not emit partial results or punctuation.
/// Hints are snapshotted at start, bounded to 64 known terms of ten tokens each,
/// and never retained in shared decoder state or written to diagnostics.
pub struct SeacoSpeechRecognizer {
    decoder: Arc<dyn SeacoDecoder>,
    vocabulary: Arc<Vocabulary>,
}

impl SeacoSpeechRecognizer {
    pub fn load(directory: &Path) -> Result<Self, SpeechRecognitionError> {
        let files = model::ModelFiles::validate(directory)?;
        let vocabulary = Arc::new(Vocabulary::load(&files.tokens, &files.segmentation)?);
        let frontend = frontend::Frontend::load(&files.cmvn)?;
        Ok(Self {
            decoder: Arc::new(runtime::OrtDecoder::load(
                &files,
                frontend,
                Arc::clone(&vocabulary),
            )?),
            vocabulary,
        })
    }
}

impl StreamingSpeechRecognizer for SeacoSpeechRecognizer {
    fn start(
        &self,
        hints: SpeechRecognitionHints,
        _on_partial: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Result<Box<dyn StreamingRecognitionSession>, SpeechRecognitionError> {
        Ok(Box::new(SeacoSession {
            decoder: Arc::clone(&self.decoder),
            hotwords: self.vocabulary.hotwords(hints.terms()),
            samples: Mutex::new(Vec::new()),
        }))
    }
}

/// Compiles isolated session hints and decodes one bounded audio segment.
/// Implementations must not retain bias embeddings between calls or log content.
trait SeacoDecoder: Send + Sync {
    fn embed(&self, hotwords: &Hotwords) -> Result<Vec<f32>, SpeechRecognitionError>;
    fn decode(&self, samples: &[i16], bias: &[f32]) -> Result<String, SpeechRecognitionError>;
}

struct SeacoSession {
    decoder: Arc<dyn SeacoDecoder>,
    hotwords: Hotwords,
    samples: Mutex<Vec<i16>>,
}

impl StreamingRecognitionSession for SeacoSession {
    fn push_audio(&self, samples: Vec<i16>) -> Result<(), SpeechRecognitionError> {
        self.samples.lock().map_err(lock_error)?.extend(samples);
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<String, SpeechRecognitionError> {
        let samples = self.samples.into_inner().map_err(lock_error)?;
        if samples.is_empty() {
            return Err(protocol("received no audio"));
        }
        let bias = self.decoder.embed(&self.hotwords)?;
        let mut transcript = String::new();
        for segment in samples.chunks(MAX_SEGMENT_SAMPLES) {
            append_segment(&mut transcript, &self.decoder.decode(segment, &bias)?);
        }
        if transcript.is_empty() {
            Err(protocol("returned an empty final transcript"))
        } else {
            Ok(transcript)
        }
    }

    fn cancel(self: Box<Self>) {}
}

fn append_segment(transcript: &mut String, segment: &str) {
    let segment = segment.trim();
    if transcript
        .chars()
        .last()
        .zip(segment.chars().next())
        .is_some_and(|(left, right)| left.is_ascii_alphanumeric() && right.is_ascii_alphanumeric())
    {
        transcript.push(' ');
    }
    transcript.push_str(segment);
}

fn protocol(message: &str) -> SpeechRecognitionError {
    SpeechRecognitionError::Protocol(format!("SeACo {message}"))
}

fn lock_error<T>(_error: std::sync::PoisonError<T>) -> SpeechRecognitionError {
    SpeechRecognitionError::Transport("SeACo runtime lock was poisoned".to_owned())
}

fn read_text(path: &Path) -> Result<String, SpeechRecognitionError> {
    std::fs::read_to_string(path).map_err(|_| protocol("could not read a model configuration file"))
}
