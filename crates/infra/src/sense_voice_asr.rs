use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use sherpa_onnx::{
    OfflineRecognizer, OfflineRecognizerConfig, OfflineSenseVoiceModelConfig, SileroVadModelConfig,
    VadModelConfig, VoiceActivityDetector,
};
use template_app::{
    SpeechRecognitionError, SpeechRecognitionHints, StreamingRecognitionSession,
    StreamingSpeechRecognizer,
};

const SAMPLE_RATE: i32 = 16_000;
const MAX_SEGMENT_SAMPLES: usize = 30 * SAMPLE_RATE as usize;
const MODEL_FILE: &str = "model.int8.onnx";
const MODEL_BYTES: u64 = 239_233_841;
const TOKENS_FILE: &str = "tokens.txt";
const TOKENS_BYTES: u64 = 315_894;
const VAD_FILE: &str = "silero_vad.onnx";
const VAD_BYTES: u64 = 643_854;
const VAD_PADDING_SAMPLES: usize = 3 * SAMPLE_RATE as usize / 10;

/// Loads the pinned SenseVoiceSmall INT8 artifact for final-result decoding.
///
/// The recognizer auto-detects one of the model's five languages and enables
/// native inverse text normalization, including the model's punctuation output.
pub struct SenseVoiceSpeechRecognizer {
    decoder: Arc<dyn SenseVoiceDecoder>,
    segmenter: Arc<dyn SpeechSegmenter>,
}

impl SenseVoiceSpeechRecognizer {
    pub fn load(model_directory: &Path) -> Result<Self, SpeechRecognitionError> {
        let files = SenseVoiceModelFiles::validate(model_directory)?;
        let mut config = OfflineRecognizerConfig::default();
        config.feat_config.feature_dim = 80;
        config.model_config.sense_voice = OfflineSenseVoiceModelConfig {
            model: Some(path_text(&files.model)?),
            language: Some("auto".to_owned()),
            use_itn: true,
        };
        config.model_config.tokens = Some(path_text(&files.tokens)?);
        config.model_config.num_threads = 4;
        config.model_config.provider = Some("cpu".to_owned());
        let recognizer = OfflineRecognizer::create(&config).ok_or_else(|| {
            SpeechRecognitionError::Protocol(
                "the SenseVoice runtime could not load the pinned model files".to_owned(),
            )
        })?;
        Ok(Self {
            decoder: Arc::new(SherpaSenseVoiceDecoder {
                recognizer: Mutex::new(recognizer),
            }),
            segmenter: Arc::new(SileroSpeechSegmenter {
                model: path_text(&files.vad)?,
            }),
        })
    }

    #[cfg(test)]
    fn with_runtime(
        decoder: Arc<dyn SenseVoiceDecoder>,
        segmenter: Arc<dyn SpeechSegmenter>,
    ) -> Self {
        Self { decoder, segmenter }
    }
}

impl StreamingSpeechRecognizer for SenseVoiceSpeechRecognizer {
    fn start(
        &self,
        _hints: SpeechRecognitionHints,
        _on_partial: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Result<Box<dyn StreamingRecognitionSession>, SpeechRecognitionError> {
        Ok(Box::new(SenseVoiceSession {
            decoder: Arc::clone(&self.decoder),
            segmenter: Arc::clone(&self.segmenter),
            samples: Mutex::new(Vec::new()),
        }))
    }
}

struct SenseVoiceModelFiles {
    model: PathBuf,
    tokens: PathBuf,
    vad: PathBuf,
}

impl SenseVoiceModelFiles {
    fn validate(directory: &Path) -> Result<Self, SpeechRecognitionError> {
        if !directory.is_dir() {
            return Err(SpeechRecognitionError::NotConfigured);
        }
        Ok(Self {
            model: validate_file(directory, MODEL_FILE, MODEL_BYTES)?,
            tokens: validate_file(directory, TOKENS_FILE, TOKENS_BYTES)?,
            vad: validate_file(directory, VAD_FILE, VAD_BYTES)?,
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
            "{} does not match the pinned SenseVoice artifact",
            path.display()
        )));
    }
    Ok(path)
}

fn path_text(path: &Path) -> Result<String, SpeechRecognitionError> {
    path.to_str().map(ToOwned::to_owned).ok_or_else(|| {
        SpeechRecognitionError::Protocol(format!(
            "the SenseVoice model path is not valid Unicode: {}",
            path.display()
        ))
    })
}

trait SenseVoiceDecoder: Send + Sync {
    fn decode(&self, samples: &[i16]) -> Result<String, SpeechRecognitionError>;
}

trait SpeechSegmenter: Send + Sync {
    fn segments(&self, samples: &[i16]) -> Result<Vec<Vec<i16>>, SpeechRecognitionError>;
}

struct SileroSpeechSegmenter {
    model: String,
}

impl SpeechSegmenter for SileroSpeechSegmenter {
    fn segments(&self, samples: &[i16]) -> Result<Vec<Vec<i16>>, SpeechRecognitionError> {
        let config = VadModelConfig {
            silero_vad: SileroVadModelConfig {
                model: Some(self.model.clone()),
                threshold: 0.5,
                min_silence_duration: 0.5,
                min_speech_duration: 0.25,
                window_size: 512,
                max_speech_duration: 30.0,
            },
            sample_rate: SAMPLE_RATE,
            num_threads: 1,
            provider: Some("cpu".to_owned()),
            debug: false,
            ..Default::default()
        };
        let vad = VoiceActivityDetector::create(&config, 60.0).ok_or_else(|| {
            SpeechRecognitionError::Protocol(
                "the SenseVoice VAD runtime could not load its pinned model".to_owned(),
            )
        })?;
        let waveform = pcm_f32(samples);
        let mut ranges = Vec::new();
        for chunk in waveform.chunks(512) {
            vad.accept_waveform(chunk);
            collect_vad_ranges(&vad, waveform.len(), &mut ranges);
        }
        vad.flush();
        collect_vad_ranges(&vad, waveform.len(), &mut ranges);
        Ok(ranges
            .into_iter()
            .map(|(start, end)| pcm_i16(&waveform[start..end]))
            .collect())
    }
}

fn collect_vad_ranges(
    vad: &VoiceActivityDetector,
    sample_count: usize,
    ranges: &mut Vec<(usize, usize)>,
) {
    while let Some(segment) = vad.front() {
        let detected_start = segment.start().max(0) as usize;
        let detected_end = detected_start
            .saturating_add(segment.samples().len())
            .min(sample_count);
        let start = detected_start.saturating_sub(VAD_PADDING_SAMPLES);
        let end = detected_end
            .saturating_add(VAD_PADDING_SAMPLES)
            .min(sample_count);
        if let Some((_, previous_end)) = ranges.last_mut()
            && *previous_end >= start
        {
            *previous_end = (*previous_end).max(end);
        } else {
            ranges.push((start, end));
        }
        vad.pop();
    }
}

fn pcm_i16(samples: &[f32]) -> Vec<i16> {
    samples
        .iter()
        .map(|sample| (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16)
        .collect()
}

struct SherpaSenseVoiceDecoder {
    recognizer: Mutex<OfflineRecognizer>,
}

impl SenseVoiceDecoder for SherpaSenseVoiceDecoder {
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

struct SenseVoiceSession {
    decoder: Arc<dyn SenseVoiceDecoder>,
    segmenter: Arc<dyn SpeechSegmenter>,
    samples: Mutex<Vec<i16>>,
}

impl StreamingRecognitionSession for SenseVoiceSession {
    fn push_audio(&self, samples: Vec<i16>) -> Result<(), SpeechRecognitionError> {
        self.samples.lock().map_err(lock_error)?.extend(samples);
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<String, SpeechRecognitionError> {
        let samples = self.samples.into_inner().map_err(lock_error)?;
        if samples.is_empty() {
            return Err(SpeechRecognitionError::Protocol(
                "SenseVoice received no audio".to_owned(),
            ));
        }
        let mut transcript = String::new();
        for speech in self.segmenter.segments(&samples)? {
            for segment in speech.chunks(MAX_SEGMENT_SAMPLES) {
                append_segment(&mut transcript, &self.decoder.decode(segment)?);
            }
        }
        if transcript.is_empty() {
            Err(SpeechRecognitionError::Protocol(
                "SenseVoice returned an empty final transcript".to_owned(),
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

fn pcm_f32(samples: &[i16]) -> Vec<f32> {
    samples
        .iter()
        .map(|sample| f32::from(*sample) / 32_768.0)
        .collect()
}

fn lock_error(error: impl std::fmt::Display) -> SpeechRecognitionError {
    SpeechRecognitionError::Transport(format!("SenseVoice runtime lock failed: {error}"))
}

#[cfg(test)]
#[allow(clippy::panic_in_result_fn)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    struct FakeDecoder {
        calls: Arc<AtomicUsize>,
        text: &'static str,
    }

    struct FixedSegmenter {
        segment_samples: usize,
    }

    struct NoSpeechSegmenter;

    impl SpeechSegmenter for NoSpeechSegmenter {
        fn segments(&self, _samples: &[i16]) -> Result<Vec<Vec<i16>>, SpeechRecognitionError> {
            Ok(Vec::new())
        }
    }

    impl SpeechSegmenter for FixedSegmenter {
        fn segments(&self, samples: &[i16]) -> Result<Vec<Vec<i16>>, SpeechRecognitionError> {
            Ok(samples
                .chunks(self.segment_samples)
                .map(<[i16]>::to_vec)
                .collect())
        }
    }

    impl SenseVoiceDecoder for FakeDecoder {
        fn decode(&self, _samples: &[i16]) -> Result<String, SpeechRecognitionError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Ok(self.text.to_owned())
        }
    }

    #[test]
    fn long_audio_is_split_and_native_punctuation_is_preserved()
    -> Result<(), SpeechRecognitionError> {
        let calls = Arc::new(AtomicUsize::new(0));
        let recognizer = SenseVoiceSpeechRecognizer::with_runtime(
            Arc::new(FakeDecoder {
                calls: Arc::clone(&calls),
                text: "今天是 2026 年 8 月 4 日。",
            }),
            Arc::new(FixedSegmenter {
                segment_samples: MAX_SEGMENT_SAMPLES,
            }),
        );
        let session = recognizer.start(SpeechRecognitionHints::default(), Arc::new(|_| {}))?;
        session.push_audio(vec![1; MAX_SEGMENT_SAMPLES + 1_600])?;

        assert_eq!(
            "今天是 2026 年 8 月 4 日。今天是 2026 年 8 月 4 日。",
            session.finish()?
        );
        assert_eq!(2, calls.load(Ordering::Relaxed));
        Ok(())
    }

    #[test]
    fn empty_audio_and_cancel_do_not_decode() -> Result<(), SpeechRecognitionError> {
        let calls = Arc::new(AtomicUsize::new(0));
        let recognizer = SenseVoiceSpeechRecognizer::with_runtime(
            Arc::new(FakeDecoder {
                calls: Arc::clone(&calls),
                text: "unused",
            }),
            Arc::new(FixedSegmenter {
                segment_samples: MAX_SEGMENT_SAMPLES,
            }),
        );
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
    fn audio_without_detected_speech_is_rejected_without_decoding()
    -> Result<(), SpeechRecognitionError> {
        let calls = Arc::new(AtomicUsize::new(0));
        let recognizer = SenseVoiceSpeechRecognizer::with_runtime(
            Arc::new(FakeDecoder {
                calls: Arc::clone(&calls),
                text: "unused",
            }),
            Arc::new(NoSpeechSegmenter),
        );
        let session = recognizer.start(SpeechRecognitionHints::default(), Arc::new(|_| {}))?;
        session.push_audio(vec![0; SAMPLE_RATE as usize])?;

        assert!(matches!(
            session.finish(),
            Err(SpeechRecognitionError::Protocol(_))
        ));
        assert_eq!(0, calls.load(Ordering::Relaxed));
        Ok(())
    }
}
