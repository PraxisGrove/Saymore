use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use sherpa_onnx::{OnlineRecognizer, OnlineRecognizerConfig, OnlineStream};
use template_app::{
    SpeechRecognitionError, SpeechRecognitionHints, StreamingRecognitionSession,
    StreamingSpeechRecognizer,
};

const SAMPLE_RATE: i32 = 16_000;
const TAIL_PADDING_SAMPLES: usize = 4_800;
const ENCODER_FILE: &str = "encoder.int8.onnx";
const ENCODER_BYTES: u64 = 165_462_184;
const DECODER_FILE: &str = "decoder.int8.onnx";
const DECODER_BYTES: u64 = 71_664_561;
const TOKENS_FILE: &str = "tokens.txt";
const TOKENS_BYTES: u64 = 75_756;

/// Loads the pinned streaming Paraformer INT8 artifact once and creates isolated sessions.
pub struct ParaformerSpeechRecognizer {
    decoder: Arc<dyn ParaformerDecoder>,
}

impl ParaformerSpeechRecognizer {
    pub fn load(model_directory: &Path) -> Result<Self, SpeechRecognitionError> {
        let files = ParaformerModelFiles::validate(model_directory)?;
        let mut config = OnlineRecognizerConfig::default();
        config.model_config.paraformer.encoder = Some(path_text(&files.encoder)?);
        config.model_config.paraformer.decoder = Some(path_text(&files.decoder)?);
        config.model_config.tokens = Some(path_text(&files.tokens)?);
        config.model_config.num_threads = 4;
        config.model_config.provider = Some("cpu".to_owned());
        config.decoding_method = Some("greedy_search".to_owned());
        let recognizer = OnlineRecognizer::create(&config).ok_or_else(|| {
            SpeechRecognitionError::Protocol(
                "the Paraformer runtime could not load the pinned model files".to_owned(),
            )
        })?;
        Ok(Self {
            decoder: Arc::new(SherpaParaformerDecoder {
                recognizer: Arc::new(recognizer),
            }),
        })
    }

    #[cfg(test)]
    fn with_decoder(decoder: Arc<dyn ParaformerDecoder>) -> Self {
        Self { decoder }
    }
}

impl StreamingSpeechRecognizer for ParaformerSpeechRecognizer {
    fn start(
        &self,
        _hints: SpeechRecognitionHints,
        on_partial: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Result<Box<dyn StreamingRecognitionSession>, SpeechRecognitionError> {
        Ok(Box::new(ParaformerSession {
            decoder: Mutex::new(ParaformerSessionDecoder {
                stream: self.decoder.create_stream(),
                last_partial: String::new(),
            }),
            on_partial,
        }))
    }
}

struct ParaformerModelFiles {
    encoder: PathBuf,
    decoder: PathBuf,
    tokens: PathBuf,
}

impl ParaformerModelFiles {
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
            "{} does not match the pinned Paraformer artifact",
            path.display()
        )));
    }
    Ok(path)
}

fn path_text(path: &Path) -> Result<String, SpeechRecognitionError> {
    path.to_str().map(ToOwned::to_owned).ok_or_else(|| {
        SpeechRecognitionError::Protocol(format!(
            "the Paraformer model path is not valid Unicode: {}",
            path.display()
        ))
    })
}

trait ParaformerDecoder: Send + Sync {
    fn create_stream(&self) -> Box<dyn ParaformerStream>;
}

trait ParaformerStream: Send {
    fn accept_audio(&mut self, samples: &[f32]);
    fn decode_ready(&mut self);
    fn current_text(&mut self) -> Option<String>;
    fn finish_input(&mut self);
}

struct SherpaParaformerDecoder {
    recognizer: Arc<OnlineRecognizer>,
}

impl ParaformerDecoder for SherpaParaformerDecoder {
    fn create_stream(&self) -> Box<dyn ParaformerStream> {
        Box::new(SherpaParaformerStream {
            recognizer: Arc::clone(&self.recognizer),
            stream: self.recognizer.create_stream(),
        })
    }
}

struct SherpaParaformerStream {
    recognizer: Arc<OnlineRecognizer>,
    stream: OnlineStream,
}

impl ParaformerStream for SherpaParaformerStream {
    fn accept_audio(&mut self, samples: &[f32]) {
        self.stream.accept_waveform(SAMPLE_RATE, samples);
    }

    fn decode_ready(&mut self) {
        while self.recognizer.is_ready(&self.stream) {
            self.recognizer.decode(&self.stream);
        }
    }

    fn current_text(&mut self) -> Option<String> {
        self.recognizer
            .get_result(&self.stream)
            .map(|result| result.text)
    }

    fn finish_input(&mut self) {
        self.stream.set_option("is_final", "1");
        self.stream.input_finished();
    }
}

struct ParaformerSessionDecoder {
    stream: Box<dyn ParaformerStream>,
    last_partial: String,
}

struct ParaformerSession {
    decoder: Mutex<ParaformerSessionDecoder>,
    on_partial: Arc<dyn Fn(String) + Send + Sync>,
}

impl StreamingRecognitionSession for ParaformerSession {
    fn push_audio(&self, samples: Vec<i16>) -> Result<(), SpeechRecognitionError> {
        if samples.is_empty() {
            return Ok(());
        }
        let samples = pcm_f32(&samples);
        let partial = {
            let mut decoder = self.decoder.lock().map_err(lock_error)?;
            decoder.stream.accept_audio(&samples);
            decoder.stream.decode_ready();
            changed_partial(&mut decoder)
        };
        if let Some(partial) = partial {
            (self.on_partial)(partial);
        }
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<String, SpeechRecognitionError> {
        let mut decoder = self.decoder.lock().map_err(lock_error)?;
        decoder
            .stream
            .accept_audio(&[0.0_f32; TAIL_PADDING_SAMPLES]);
        decoder.stream.finish_input();
        decoder.stream.decode_ready();
        let text = decoder
            .stream
            .current_text()
            .unwrap_or_default()
            .trim()
            .to_owned();
        if text.is_empty() {
            Err(SpeechRecognitionError::Protocol(
                "Paraformer returned an empty final transcript".to_owned(),
            ))
        } else {
            Ok(text)
        }
    }

    fn cancel(self: Box<Self>) {}
}

fn changed_partial(decoder: &mut ParaformerSessionDecoder) -> Option<String> {
    let text = decoder.stream.current_text()?.trim().to_owned();
    if text.is_empty() || text == decoder.last_partial {
        None
    } else {
        decoder.last_partial.clone_from(&text);
        Some(text)
    }
}

fn pcm_f32(samples: &[i16]) -> Vec<f32> {
    samples
        .iter()
        .map(|sample| f32::from(*sample) / 32_768.0)
        .collect()
}

fn lock_error(error: impl std::fmt::Display) -> SpeechRecognitionError {
    SpeechRecognitionError::Transport(format!("Paraformer session lock failed: {error}"))
}

#[cfg(test)]
#[allow(clippy::panic_in_result_fn)]
mod tests {
    use std::{
        collections::VecDeque,
        fs::File,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use super::*;

    #[test]
    fn fixed_artifact_validation_requires_all_exact_file_sizes()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        create_sparse_file(&directory.path().join(ENCODER_FILE), ENCODER_BYTES)?;
        create_sparse_file(&directory.path().join(DECODER_FILE), DECODER_BYTES)?;
        create_sparse_file(&directory.path().join(TOKENS_FILE), TOKENS_BYTES)?;

        let files = ParaformerModelFiles::validate(directory.path())?;
        assert_eq!(directory.path().join(ENCODER_FILE), files.encoder);

        File::options()
            .write(true)
            .open(directory.path().join(TOKENS_FILE))?
            .set_len(TOKENS_BYTES - 1)?;
        assert!(matches!(
            ParaformerModelFiles::validate(directory.path()),
            Err(SpeechRecognitionError::Protocol(_))
        ));
        Ok(())
    }

    #[test]
    fn session_emits_changed_partials_and_finishes_in_order() -> Result<(), SpeechRecognitionError>
    {
        let state = Arc::new(Mutex::new(FakeState {
            results: VecDeque::from([
                Some(" partial ".to_owned()),
                Some("partial".to_owned()),
                Some(" final text ".to_owned()),
            ]),
            ..FakeState::default()
        }));
        let recognizer = ParaformerSpeechRecognizer::with_decoder(Arc::new(FakeDecoder {
            state: Arc::clone(&state),
        }));
        let partials = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&partials);
        let session = recognizer.start(
            SpeechRecognitionHints::default(),
            Arc::new(move |partial| {
                if let Ok(mut partials) = captured.lock() {
                    partials.push(partial);
                }
            }),
        )?;

        session.push_audio(vec![i16::MIN, 0, i16::MAX])?;
        session.push_audio(vec![1])?;
        assert_eq!("final text", session.finish()?);
        assert_eq!(vec!["partial"], *partials.lock().map_err(lock_error)?);

        let state = state.lock().map_err(lock_error)?;
        assert_eq!(3, state.accepted.len());
        assert_eq!(
            vec![-1.0, 0.0, f32::from(i16::MAX) / 32_768.0],
            state.accepted[0]
        );
        assert_eq!(TAIL_PADDING_SAMPLES, state.accepted[2].len());
        assert_eq!(
            vec![
                "accept", "decode", "result", "accept", "decode", "result", "accept", "finish",
                "decode", "result"
            ],
            state.actions
        );
        Ok(())
    }

    #[test]
    fn empty_final_result_is_a_protocol_error() -> Result<(), SpeechRecognitionError> {
        let recognizer = fake_recognizer(VecDeque::from([Some(" ".to_owned())]));
        let session = recognizer.start(SpeechRecognitionHints::default(), Arc::new(|_| {}))?;

        assert!(matches!(
            session.finish(),
            Err(SpeechRecognitionError::Protocol(_))
        ));
        Ok(())
    }

    #[test]
    fn cancel_drops_the_stream_without_finishing_it() -> Result<(), SpeechRecognitionError> {
        let state = Arc::new(Mutex::new(FakeState::default()));
        let dropped = Arc::new(AtomicUsize::new(0));
        let recognizer = ParaformerSpeechRecognizer::with_decoder(Arc::new(FakeDecoderWithDrop {
            state: Arc::clone(&state),
            dropped: Arc::clone(&dropped),
        }));
        let session = recognizer.start(SpeechRecognitionHints::default(), Arc::new(|_| {}))?;

        session.cancel();

        assert_eq!(1, dropped.load(Ordering::Acquire));
        assert!(state.lock().map_err(lock_error)?.actions.is_empty());
        Ok(())
    }

    fn create_sparse_file(path: &Path, bytes: u64) -> Result<(), std::io::Error> {
        let file = File::create(path)?;
        file.set_len(bytes)
    }

    fn fake_recognizer(results: VecDeque<Option<String>>) -> ParaformerSpeechRecognizer {
        ParaformerSpeechRecognizer::with_decoder(Arc::new(FakeDecoder {
            state: Arc::new(Mutex::new(FakeState {
                results,
                ..FakeState::default()
            })),
        }))
    }

    #[derive(Default)]
    struct FakeState {
        actions: Vec<&'static str>,
        accepted: Vec<Vec<f32>>,
        results: VecDeque<Option<String>>,
    }

    struct FakeDecoder {
        state: Arc<Mutex<FakeState>>,
    }

    impl ParaformerDecoder for FakeDecoder {
        fn create_stream(&self) -> Box<dyn ParaformerStream> {
            Box::new(FakeStream {
                state: Arc::clone(&self.state),
                dropped: None,
            })
        }
    }

    struct FakeDecoderWithDrop {
        state: Arc<Mutex<FakeState>>,
        dropped: Arc<AtomicUsize>,
    }

    impl ParaformerDecoder for FakeDecoderWithDrop {
        fn create_stream(&self) -> Box<dyn ParaformerStream> {
            Box::new(FakeStream {
                state: Arc::clone(&self.state),
                dropped: Some(Arc::clone(&self.dropped)),
            })
        }
    }

    struct FakeStream {
        state: Arc<Mutex<FakeState>>,
        dropped: Option<Arc<AtomicUsize>>,
    }

    impl ParaformerStream for FakeStream {
        fn accept_audio(&mut self, samples: &[f32]) {
            if let Ok(mut state) = self.state.lock() {
                state.actions.push("accept");
                state.accepted.push(samples.to_vec());
            }
        }

        fn decode_ready(&mut self) {
            if let Ok(mut state) = self.state.lock() {
                state.actions.push("decode");
            }
        }

        fn current_text(&mut self) -> Option<String> {
            self.state.lock().ok().and_then(|mut state| {
                state.actions.push("result");
                state.results.pop_front().flatten()
            })
        }

        fn finish_input(&mut self) {
            if let Ok(mut state) = self.state.lock() {
                state.actions.push("finish");
            }
        }
    }

    impl Drop for FakeStream {
        fn drop(&mut self) {
            if let Some(dropped) = &self.dropped {
                dropped.fetch_add(1, Ordering::AcqRel);
            }
        }
    }
}
