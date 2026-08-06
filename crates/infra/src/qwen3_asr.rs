use std::{
    fs,
    path::Path,
    sync::{Arc, Mutex},
};

use sherpa_onnx::{OfflineQwen3ASRModelConfig, OfflineRecognizer, OfflineRecognizerConfig};
use template_app::{
    SpeechRecognitionError, SpeechRecognitionHints, StreamingRecognitionSession,
    StreamingSpeechRecognizer,
};

const SAMPLE_RATE: i32 = 16_000;
const MAX_SEGMENT_SAMPLES: usize = 30 * SAMPLE_RATE as usize;
const SEGMENT_OVERLAP_SAMPLES: usize = SAMPLE_RATE as usize;
const MODEL_FILES: [(&str, u64); 6] = [
    ("conv_frontend.onnx", 48_080_441),
    ("encoder.int8.onnx", 314_222_162),
    ("decoder.int8.onnx", 2_037_458_645),
    ("tokenizer/merges.txt", 1_671_853),
    ("tokenizer/tokenizer_config.json", 12_487),
    ("tokenizer/vocab.json", 2_776_833),
];

/// Loads the pinned Qwen3-ASR 1.7B INT8 artifact for final-result decoding.
pub struct Qwen3AsrSpeechRecognizer {
    decoder: Arc<dyn Qwen3Decoder>,
}

impl Qwen3AsrSpeechRecognizer {
    pub fn load(model_directory: &Path) -> Result<Self, SpeechRecognitionError> {
        validate_model_files(model_directory)?;
        let mut config = OfflineRecognizerConfig::default();
        config.feat_config.feature_dim = 80;
        config.model_config.qwen3_asr = OfflineQwen3ASRModelConfig {
            conv_frontend: Some(path_text(&model_directory.join("conv_frontend.onnx"))?),
            encoder: Some(path_text(&model_directory.join("encoder.int8.onnx"))?),
            decoder: Some(path_text(&model_directory.join("decoder.int8.onnx"))?),
            tokenizer: Some(path_text(&model_directory.join("tokenizer"))?),
            max_total_len: 512,
            max_new_tokens: 512,
            temperature: 0.000_001,
            top_p: 0.8,
            seed: 42,
            hotwords: None,
        };
        config.model_config.num_threads = 4;
        config.model_config.provider = Some("cpu".to_owned());
        config.decoding_method = Some("greedy_search".to_owned());
        let recognizer = OfflineRecognizer::create(&config).ok_or_else(|| {
            SpeechRecognitionError::Protocol(
                "the Qwen3-ASR runtime could not load the pinned model files".to_owned(),
            )
        })?;
        Ok(Self {
            decoder: Arc::new(SherpaQwen3Decoder {
                recognizer: Mutex::new(recognizer),
            }),
        })
    }

    #[cfg(test)]
    fn with_decoder(decoder: Arc<dyn Qwen3Decoder>) -> Self {
        Self { decoder }
    }
}

impl StreamingSpeechRecognizer for Qwen3AsrSpeechRecognizer {
    fn start(
        &self,
        _hints: SpeechRecognitionHints,
        _on_partial: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Result<Box<dyn StreamingRecognitionSession>, SpeechRecognitionError> {
        Ok(Box::new(Qwen3Session {
            decoder: Arc::clone(&self.decoder),
            samples: Mutex::new(Vec::new()),
        }))
    }
}

fn validate_model_files(directory: &Path) -> Result<(), SpeechRecognitionError> {
    if !directory.is_dir() {
        return Err(SpeechRecognitionError::NotConfigured);
    }
    for (name, expected_bytes) in MODEL_FILES {
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
                "{} does not match the pinned Qwen3-ASR artifact",
                path.display()
            )));
        }
    }
    Ok(())
}

fn path_text(path: &Path) -> Result<String, SpeechRecognitionError> {
    path.to_str().map(ToOwned::to_owned).ok_or_else(|| {
        SpeechRecognitionError::Protocol(format!(
            "the Qwen3-ASR model path is not valid Unicode: {}",
            path.display()
        ))
    })
}

trait Qwen3Decoder: Send + Sync {
    fn decode(&self, samples: &[i16]) -> Result<String, SpeechRecognitionError>;
}

struct SherpaQwen3Decoder {
    recognizer: Mutex<OfflineRecognizer>,
}

impl Qwen3Decoder for SherpaQwen3Decoder {
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

struct Qwen3Session {
    decoder: Arc<dyn Qwen3Decoder>,
    samples: Mutex<Vec<i16>>,
}

impl StreamingRecognitionSession for Qwen3Session {
    fn push_audio(&self, samples: Vec<i16>) -> Result<(), SpeechRecognitionError> {
        self.samples.lock().map_err(lock_error)?.extend(samples);
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<String, SpeechRecognitionError> {
        let samples = self.samples.into_inner().map_err(lock_error)?;
        if samples.is_empty() {
            return Err(SpeechRecognitionError::Protocol(
                "Qwen3-ASR received no audio".to_owned(),
            ));
        }
        let mut transcript = String::new();
        let mut start: usize = 0;
        loop {
            let end = start.saturating_add(MAX_SEGMENT_SAMPLES).min(samples.len());
            append_segment(&mut transcript, &self.decoder.decode(&samples[start..end])?);
            if end == samples.len() {
                break;
            }
            start = end.saturating_sub(SEGMENT_OVERLAP_SAMPLES);
        }
        if transcript.is_empty() {
            Err(SpeechRecognitionError::Protocol(
                "Qwen3-ASR returned an empty final transcript".to_owned(),
            ))
        } else {
            Ok(transcript)
        }
    }

    fn cancel(self: Box<Self>) {}
}

fn append_segment(transcript: &mut String, segment: &str) {
    let (segment, overlapped) = deduplicated_segment(transcript, speech_text(segment));
    if segment.is_empty() {
        return;
    }
    if overlapped {
        while transcript
            .chars()
            .last()
            .is_some_and(|character| !character.is_alphanumeric())
        {
            transcript.pop();
        }
    }
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

fn speech_text(segment: &str) -> &str {
    let segment = segment.trim();
    segment
        .split_once("<asr_text>")
        .map(|(_, speech)| speech.trim_start())
        .unwrap_or(segment)
}

fn deduplicated_segment<'a>(transcript: &str, segment: &'a str) -> (&'a str, bool) {
    let left = transcript
        .chars()
        .filter(|character| character.is_alphanumeric())
        .collect::<Vec<_>>();
    let right = segment
        .char_indices()
        .filter(|(_, character)| character.is_alphanumeric())
        .collect::<Vec<_>>();
    let maximum = left.len().min(right.len()).min(64);
    let overlap = (1..=maximum).rev().find_map(|length| {
        let left_start = left.len() - length;
        let minimum = if left[left_start..].iter().all(char::is_ascii) {
            8
        } else {
            3
        };
        if length < minimum {
            return None;
        }
        (0..=4.min(right.len().saturating_sub(length))).find_map(|right_start| {
            left[left_start..]
                .iter()
                .copied()
                .eq(right[right_start..right_start + length]
                    .iter()
                    .map(|(_, character)| *character))
                .then_some((right_start, length))
        })
    });
    let Some((right_start, overlap)) = overlap else {
        return (segment, false);
    };
    let (start, character) = right[right_start + overlap - 1];
    (segment[start + character.len_utf8()..].trim_start(), true)
}

fn pcm_f32(samples: &[i16]) -> Vec<f32> {
    samples
        .iter()
        .map(|sample| f32::from(*sample) / 32_768.0)
        .collect()
}

fn lock_error(error: impl std::fmt::Display) -> SpeechRecognitionError {
    SpeechRecognitionError::Transport(format!("Qwen3-ASR runtime lock failed: {error}"))
}

#[cfg(test)]
#[allow(clippy::panic_in_result_fn)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    struct FakeDecoder {
        calls: Arc<AtomicUsize>,
        decoded_samples: Arc<AtomicUsize>,
    }

    impl Qwen3Decoder for FakeDecoder {
        fn decode(&self, samples: &[i16]) -> Result<String, SpeechRecognitionError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            self.decoded_samples
                .fetch_add(samples.len(), Ordering::Relaxed);
            Ok("segment".to_owned())
        }
    }

    #[test]
    fn long_audio_is_decoded_without_losing_samples() -> Result<(), SpeechRecognitionError> {
        let decoded_samples = Arc::new(AtomicUsize::new(0));
        let recognizer = Qwen3AsrSpeechRecognizer::with_decoder(Arc::new(FakeDecoder {
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

        assert_eq!("segment segment", session.finish()?);
        assert_eq!(
            total_samples + SEGMENT_OVERLAP_SAMPLES,
            decoded_samples.load(Ordering::Relaxed)
        );
        assert_eq!(0, partials.load(Ordering::Relaxed));
        Ok(())
    }

    #[test]
    fn empty_audio_and_cancel_do_not_decode() -> Result<(), SpeechRecognitionError> {
        let calls = Arc::new(AtomicUsize::new(0));
        let recognizer = Qwen3AsrSpeechRecognizer::with_decoder(Arc::new(FakeDecoder {
            calls: Arc::clone(&calls),
            decoded_samples: Arc::new(AtomicUsize::new(0)),
        }));
        let empty = recognizer.start(SpeechRecognitionHints::default(), Arc::new(|_| {}))?;
        assert!(matches!(
            empty.finish(),
            Err(SpeechRecognitionError::Protocol(_))
        ));
        let cancelled = recognizer.start(SpeechRecognitionHints::default(), Arc::new(|_| {}))?;
        cancelled.push_audio(vec![1; 1_600])?;
        cancelled.cancel();
        assert_eq!(0, calls.load(Ordering::Relaxed));
        Ok(())
    }

    #[test]
    fn joins_chinese_without_spaces_and_english_with_spaces() {
        let mut chinese = "你好".to_owned();
        append_segment(&mut chinese, "世界");
        assert_eq!("你好世界", chinese);
        let mut english = "hello".to_owned();
        append_segment(&mut english, "world");
        assert_eq!("hello world", english);
    }

    #[test]
    fn removes_a_repeated_phrase_at_a_segment_boundary() {
        let mut transcript = "这对我来说是至高无上的荣誉，但是。是一个大一的新生".to_owned();
        append_segment(
            &mut transcript,
            "但是一个大一的新生，一个从县城出来的年轻人",
        );
        assert_eq!(
            "这对我来说是至高无上的荣誉，但是。是一个大一的新生，一个从县城出来的年轻人",
            transcript
        );
    }

    #[test]
    fn removes_qwen_control_metadata_before_joining_segments() {
        let mut transcript = "第一段。".to_owned();

        append_segment(&mut transcript, "language Chinese<asr_text>第二段。");

        assert_eq!("第一段。第二段。", transcript);
    }

    #[test]
    fn replaces_old_boundary_punctuation_after_overlap() {
        let mut transcript = "朋友们，晚上好。".to_owned();
        append_segment(&mut transcript, "晚上好！欢迎大家");
        assert_eq!("朋友们，晚上好！欢迎大家", transcript);
    }

    #[test]
    fn validates_the_nested_pinned_layout() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        for (name, bytes) in MODEL_FILES {
            let path: PathBuf = directory.path().join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let file = fs::File::create(path)?;
            file.set_len(bytes)?;
        }
        assert!(validate_model_files(directory.path()).is_ok());
        Ok(())
    }
}
