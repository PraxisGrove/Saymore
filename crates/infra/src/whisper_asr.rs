use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig, OfflineWhisperModelConfig};
use template_app::{
    SpeechRecognitionError, SpeechRecognitionHints, StreamingRecognitionSession,
    StreamingSpeechRecognizer,
};

const SAMPLE_RATE: i32 = 16_000;
const MAX_SEGMENT_SAMPLES: usize = 29 * SAMPLE_RATE as usize;
const ENCODER_FILE: &str = "turbo-encoder.int8.onnx";
const ENCODER_BYTES: u64 = 674_716_297;
const DECODER_FILE: &str = "turbo-decoder.int8.onnx";
const DECODER_BYTES: u64 = 361_080_764;
const TOKENS_FILE: &str = "turbo-tokens.txt";
const TOKENS_BYTES: u64 = 816_730;

/// Loads the pinned Whisper large-v3-turbo INT8 artifact for final-result decoding.
pub struct WhisperSpeechRecognizer {
    decoder: Arc<dyn WhisperDecoder>,
}

impl WhisperSpeechRecognizer {
    pub fn load(model_directory: &Path) -> Result<Self, SpeechRecognitionError> {
        let files = WhisperModelFiles::validate(model_directory)?;
        let mut config = OfflineRecognizerConfig::default();
        config.feat_config.feature_dim = 128;
        config.model_config.whisper = OfflineWhisperModelConfig {
            encoder: Some(path_text(&files.encoder)?),
            decoder: Some(path_text(&files.decoder)?),
            language: None,
            task: Some("transcribe".to_owned()),
            tail_paddings: -1,
            enable_token_timestamps: false,
            enable_segment_timestamps: false,
        };
        config.model_config.tokens = Some(path_text(&files.tokens)?);
        config.model_config.num_threads = 4;
        config.model_config.provider = Some("cpu".to_owned());
        let recognizer = OfflineRecognizer::create(&config).ok_or_else(|| {
            SpeechRecognitionError::Protocol(
                "the Whisper runtime could not load the pinned model files".to_owned(),
            )
        })?;
        Ok(Self {
            decoder: Arc::new(SherpaWhisperDecoder {
                recognizer: Mutex::new(recognizer),
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
        _hints: SpeechRecognitionHints,
        _on_partial: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Result<Box<dyn StreamingRecognitionSession>, SpeechRecognitionError> {
        Ok(Box::new(WhisperSession {
            decoder: Arc::clone(&self.decoder),
            samples: Mutex::new(Vec::new()),
        }))
    }
}

struct WhisperModelFiles {
    encoder: PathBuf,
    decoder: PathBuf,
    tokens: PathBuf,
}

impl WhisperModelFiles {
    fn validate(directory: &Path) -> Result<Self, SpeechRecognitionError> {
        if !directory.is_dir() {
            return Err(SpeechRecognitionError::NotConfigured);
        }
        Ok(Self {
            encoder: validate_file(directory, ENCODER_FILE, ENCODER_BYTES)?,
            decoder: validate_file(directory, DECODER_FILE, DECODER_BYTES)?,
            tokens: validate_file(directory, TOKENS_FILE, TOKENS_BYTES)?,
        })
    }
}

fn validate_file(
    directory: &Path,
    name: &str,
    expected_bytes: u64,
) -> Result<PathBuf, SpeechRecognitionError> {
    let path = directory.join(name);
    let metadata = match fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(SpeechRecognitionError::NotConfigured);
        }
        Err(error) => {
            return Err(SpeechRecognitionError::Transport(format!(
                "could not inspect {}: {error}",
                path.display()
            )));
        }
    };
    if !metadata.is_file() || metadata.len() != expected_bytes {
        return Err(SpeechRecognitionError::Protocol(format!(
            "{} does not match the pinned Whisper artifact",
            path.display()
        )));
    }
    Ok(path)
}

fn path_text(path: &Path) -> Result<String, SpeechRecognitionError> {
    path.to_str().map(ToOwned::to_owned).ok_or_else(|| {
        SpeechRecognitionError::Protocol(format!(
            "the Whisper model path is not valid Unicode: {}",
            path.display()
        ))
    })
}

trait WhisperDecoder: Send + Sync {
    fn decode(&self, samples: &[i16]) -> Result<String, SpeechRecognitionError>;
}

struct SherpaWhisperDecoder {
    recognizer: Mutex<OfflineRecognizer>,
}

impl WhisperDecoder for SherpaWhisperDecoder {
    fn decode(&self, samples: &[i16]) -> Result<String, SpeechRecognitionError> {
        let recognizer = self.recognizer.lock().map_err(lock_error)?;
        let stream = recognizer.create_stream();
        stream.accept_waveform(SAMPLE_RATE, &pcm_f32(samples));
        recognizer.decode(&stream);
        Ok(stream
            .get_result()
            .map(|result| result.text)
            .unwrap_or_default())
    }
}

struct WhisperSession {
    decoder: Arc<dyn WhisperDecoder>,
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
            append_segment(&mut transcript, &self.decoder.decode(segment)?);
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

fn lock_error(error: impl std::fmt::Display) -> SpeechRecognitionError {
    SpeechRecognitionError::Transport(format!("Whisper runtime lock failed: {error}"))
}

#[cfg(test)]
#[allow(clippy::panic_in_result_fn)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[test]
    fn long_audio_is_decoded_without_losing_samples() -> Result<(), SpeechRecognitionError> {
        let decoded_samples = Arc::new(AtomicUsize::new(0));
        let recognizer = WhisperSpeechRecognizer::with_decoder(Arc::new(FakeDecoder {
            calls: Arc::new(AtomicUsize::new(0)),
            decoded_samples: Arc::clone(&decoded_samples),
        }));
        let partials = Arc::new(AtomicUsize::new(0));
        let captured = Arc::clone(&partials);
        let session = recognizer.start(
            SpeechRecognitionHints::default(),
            Arc::new(move |_| {
                captured.fetch_add(1, Ordering::Relaxed);
            }),
        )?;
        let total_samples = MAX_SEGMENT_SAMPLES + 1_600;

        session.push_audio(vec![1; total_samples])?;
        let transcript = session.finish()?;

        assert_eq!(total_samples, decoded_samples.load(Ordering::Relaxed));
        assert_eq!("segment segment", transcript);
        assert_eq!(0, partials.load(Ordering::Relaxed));
        Ok(())
    }

    #[test]
    fn cancellation_does_not_decode_buffered_audio() -> Result<(), SpeechRecognitionError> {
        let calls = Arc::new(AtomicUsize::new(0));
        let recognizer = WhisperSpeechRecognizer::with_decoder(Arc::new(FakeDecoder {
            calls: Arc::clone(&calls),
            decoded_samples: Arc::new(AtomicUsize::new(0)),
        }));
        let session = recognizer.start(SpeechRecognitionHints::default(), Arc::new(|_| {}))?;
        session.push_audio(vec![1; 1_600])?;

        session.cancel();

        assert_eq!(0, calls.load(Ordering::Relaxed));
        Ok(())
    }

    #[test]
    fn empty_audio_is_rejected_without_decoding() -> Result<(), SpeechRecognitionError> {
        let calls = Arc::new(AtomicUsize::new(0));
        let recognizer = WhisperSpeechRecognizer::with_decoder(Arc::new(FakeDecoder {
            calls: Arc::clone(&calls),
            decoded_samples: Arc::new(AtomicUsize::new(0)),
        }));
        let session = recognizer.start(SpeechRecognitionHints::default(), Arc::new(|_| {}))?;

        assert!(matches!(
            session.finish(),
            Err(SpeechRecognitionError::Protocol(_))
        ));
        assert_eq!(0, calls.load(Ordering::Relaxed));
        Ok(())
    }

    #[test]
    fn joins_chinese_segments_without_inserting_spaces() {
        let mut transcript = "你好".to_owned();
        append_segment(&mut transcript, "世界");
        assert_eq!("你好世界", transcript);
    }

    struct FakeDecoder {
        calls: Arc<AtomicUsize>,
        decoded_samples: Arc<AtomicUsize>,
    }

    impl WhisperDecoder for FakeDecoder {
        fn decode(&self, samples: &[i16]) -> Result<String, SpeechRecognitionError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            self.decoded_samples
                .fetch_add(samples.len(), Ordering::Relaxed);
            Ok("segment".to_owned())
        }
    }
}
