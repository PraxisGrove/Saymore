use std::{path::Path, sync::Arc};

use template_app::{
    SpeechRecognitionError, SpeechRecognitionHints, StreamingRecognitionSession,
    StreamingSpeechRecognizer,
};

use crate::background_recognition::{BackgroundRecognition, Chunking};

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

/// CPU SeACo-Paraformer with an INT8 main graph and recognition-time vocabulary embeddings.
///
/// Loads the exact exported `model_quant.onnx`, `model_eb_quant.onnx`, `tokens.json`,
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
        on_partial: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Result<Box<dyn StreamingRecognitionSession>, SpeechRecognitionError> {
        let decoder = Arc::clone(&self.decoder);
        let hotwords = self.vocabulary.hotwords(hints.terms());
        let mut bias = None;
        BackgroundRecognition::start(
            Chunking {
                window_samples: MAX_SEGMENT_SAMPLES,
                overlap_samples: 0,
            },
            move |samples| {
                if bias.is_none() {
                    bias = Some(decoder.embed(&hotwords)?);
                }
                let embedding = bias
                    .as_deref()
                    .ok_or_else(|| protocol("vocabulary embedding is unavailable"))?;
                decoder.decode(samples, embedding)
            },
            append_segment,
            on_partial,
        )
    }
}

/// Compiles isolated session hints and decodes one bounded audio segment.
/// Implementations must not retain bias embeddings between calls or log content.
trait SeacoDecoder: Send + Sync {
    fn embed(&self, hotwords: &Hotwords) -> Result<Vec<f32>, SpeechRecognitionError>;
    fn decode(&self, samples: &[i16], bias: &[f32]) -> Result<String, SpeechRecognitionError>;
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
