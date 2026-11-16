use std::{fs, path::Path, sync::Arc};

use template_app::{
    SpeechRecognitionError, SpeechRecognitionHints, StreamingRecognitionSession,
    StreamingSpeechRecognizer,
};

use crate::{
    background_recognition::{BackgroundRecognition, Chunking},
    model_installer::{QWEN3_ASR_MODEL_BYTES, QWEN3_ASR_MODEL_FILE},
};

mod hotwords;
mod native;
mod process;

const SAMPLE_RATE: i32 = 16_000;
const MAX_SEGMENT_SAMPLES: usize = 30 * SAMPLE_RATE as usize;
const SEGMENT_OVERLAP_SAMPLES: usize = SAMPLE_RATE as usize;

/// Loads the pinned Qwen3-ASR 1.7B Q8_0 GGUF with a resident speech.cpp worker.
pub struct Qwen3AsrSpeechRecognizer {
    decoder: Arc<dyn Qwen3Decoder>,
}

impl Qwen3AsrSpeechRecognizer {
    pub fn load(model_directory: &Path) -> Result<Self, SpeechRecognitionError> {
        validate_model_file(model_directory)?;
        Ok(Self {
            decoder: Arc::new(native::NativeDecoder::load(
                &model_directory.join(QWEN3_ASR_MODEL_FILE),
            )?),
        })
    }

    /// Samples the resident inference process, separate from the desktop's memory.
    pub fn resident_memory_bytes(&self) -> Option<u64> {
        self.decoder.resident_memory_bytes()
    }

    #[cfg(test)]
    fn with_decoder(decoder: Arc<dyn Qwen3Decoder>) -> Self {
        Self { decoder }
    }
}

impl StreamingSpeechRecognizer for Qwen3AsrSpeechRecognizer {
    fn start(
        &self,
        hints: SpeechRecognitionHints,
        on_partial: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Result<Box<dyn StreamingRecognitionSession>, SpeechRecognitionError> {
        let decoder = Arc::clone(&self.decoder);
        let hotwords = hotwords::from_hints(&hints);
        BackgroundRecognition::start(
            Chunking {
                window_samples: MAX_SEGMENT_SAMPLES,
                overlap_samples: SEGMENT_OVERLAP_SAMPLES,
            },
            move |samples| decoder.decode(samples, &hotwords),
            append_segment,
            on_partial,
        )
    }
}

fn validate_model_file(directory: &Path) -> Result<(), SpeechRecognitionError> {
    let path = directory.join(QWEN3_ASR_MODEL_FILE);
    let metadata = fs::metadata(path).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => SpeechRecognitionError::NotConfigured,
        _ => SpeechRecognitionError::Transport("could not inspect the Qwen3 GGUF file".into()),
    })?;
    if !metadata.is_file() || metadata.len() != QWEN3_ASR_MODEL_BYTES {
        return Err(SpeechRecognitionError::Protocol(
            "the model does not match the pinned Qwen3-ASR Q8_0 GGUF".into(),
        ));
    }
    Ok(())
}

/// Serializes native decoding and isolates each request's vocabulary.
/// Implementations must reject failed/truncated responses rather than return partial success.
trait Qwen3Decoder: Send + Sync {
    fn decode(&self, samples: &[i16], hotwords: &str) -> Result<String, SpeechRecognitionError>;
    fn resident_memory_bytes(&self) -> Option<u64> {
        None
    }
}

fn append_segment(transcript: &mut String, segment: &str) {
    let (segment, overlapped) = deduplicated_segment(transcript, segment.trim());
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

#[cfg(test)]
fn lock_error(error: impl std::fmt::Display) -> SpeechRecognitionError {
    SpeechRecognitionError::Transport(format!("Qwen3-ASR runtime lock failed: {error}"))
}

#[cfg(test)]
#[allow(clippy::panic_in_result_fn)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    struct FakeDecoder {
        calls: Arc<AtomicUsize>,
        decoded_samples: Arc<AtomicUsize>,
    }

    impl Qwen3Decoder for FakeDecoder {
        fn decode(
            &self,
            samples: &[i16],
            _hotwords: &str,
        ) -> Result<String, SpeechRecognitionError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            self.decoded_samples
                .fetch_add(samples.len(), Ordering::Relaxed);
            Ok("segment".to_owned())
        }
    }

    struct HintDecoder(Mutex<Vec<(String, usize)>>);

    impl Qwen3Decoder for HintDecoder {
        fn decode(
            &self,
            samples: &[i16],
            hotwords: &str,
        ) -> Result<String, SpeechRecognitionError> {
            self.0
                .lock()
                .map_err(lock_error)?
                .push((hotwords.to_owned(), samples.len()));
            Ok("segment".to_owned())
        }
    }

    #[test]
    fn snapshots_hotwords_for_every_segment_without_leaking_between_sessions()
    -> Result<(), SpeechRecognitionError> {
        let decoder = Arc::new(HintDecoder(Mutex::new(Vec::new())));
        let recognizer = Qwen3AsrSpeechRecognizer::with_decoder(decoder.clone());
        let first = recognizer.start(
            SpeechRecognitionHints::from_terms(vec!["Saymore".to_owned(), "声墨".to_owned()]),
            Arc::new(|_| {}),
        )?;
        let second = recognizer.start(
            SpeechRecognitionHints::from_terms(vec!["Slint".to_owned()]),
            Arc::new(|_| {}),
        )?;
        let empty = recognizer.start(SpeechRecognitionHints::default(), Arc::new(|_| {}))?;
        first.push_audio(vec![1; MAX_SEGMENT_SAMPLES + 1])?;
        second.push_audio(vec![1; 1_600])?;
        empty.push_audio(vec![1; 1_600])?;
        second.finish()?;
        first.finish()?;
        empty.finish()?;
        // Independent sessions may now decode concurrently; preserve order within
        // each session without imposing a global finish-order on their workers.
        let mut observed = std::collections::BTreeMap::<String, Vec<usize>>::new();
        for (hints, samples) in decoder.0.lock().map_err(lock_error)?.iter() {
            observed.entry(hints.clone()).or_default().push(*samples);
        }
        assert_eq!(
            std::collections::BTreeMap::from([
                ("Slint".to_owned(), vec![1_600]),
                (
                    concat!("Saymore ", "声墨").to_owned(),
                    vec![MAX_SEGMENT_SAMPLES, 1 + SEGMENT_OVERLAP_SAMPLES]
                ),
                (String::new(), vec![1_600]),
            ]),
            observed
        );
        Ok(())
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
    fn replaces_old_boundary_punctuation_after_overlap() {
        let mut transcript = "朋友们，晚上好。".to_owned();
        append_segment(&mut transcript, "晚上好！欢迎大家");
        assert_eq!("朋友们，晚上好！欢迎大家", transcript);
    }

    #[test]
    fn rejects_missing_or_incomplete_gguf() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        assert!(matches!(
            validate_model_file(directory.path()),
            Err(SpeechRecognitionError::NotConfigured)
        ));
        let file = fs::File::create(directory.path().join(QWEN3_ASR_MODEL_FILE))?;
        file.set_len(QWEN3_ASR_MODEL_BYTES - 1)?;
        assert!(validate_model_file(directory.path()).is_err());
        file.set_len(QWEN3_ASR_MODEL_BYTES)?;
        assert!(validate_model_file(directory.path()).is_ok());
        Ok(())
    }
}
