use std::sync::Arc;

use thiserror::Error;

pub const TARGET_SAMPLE_RATE: u32 = 16_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicrophoneAuthorization {
    NotDetermined,
    Granted,
    Denied,
    Restricted,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecordingMetrics {
    pub elapsed_ms: u64,
    pub input_sample_count: usize,
    pub level: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordingStarted {
    pub input_device_name: String,
    pub used_system_fallback: bool,
}

/// An input-capable microphone exposed by the operating system.
///
/// `id` is a platform-provided stable identifier suitable for persistence. `name`
/// is presentation-only and may change when the device is renamed by the system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioInputDevice {
    pub id: String,
    pub name: String,
    pub is_system_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PcmChunk {
    pub samples: Vec<i16>,
    pub sample_rate: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PcmRecording {
    pub samples: Vec<i16>,
    pub sample_rate: u32,
    pub channels: u16,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RecordingError {
    #[error("microphone permission is required")]
    PermissionDenied,
    #[error("no default microphone is available")]
    NoInputDevice,
    #[error("recording is already active")]
    AlreadyRecording,
    #[error("recording is not active")]
    NotRecording,
    #[error("the microphone sample format is unsupported: {0}")]
    UnsupportedSampleFormat(String),
    #[error("audio capture failed: {0}")]
    Capture(String),
}

/// Captures one in-memory microphone session and returns normalized PCM audio.
///
/// Implementations are expected to use the current default input device, report
/// live metrics without blocking the audio callback, and release the device when
/// `stop` returns.
pub trait AudioRecorder: Send {
    fn start(
        &mut self,
        on_metrics: Arc<dyn Fn(RecordingMetrics) + Send + Sync>,
        on_audio_chunk: Arc<dyn Fn(PcmChunk) + Send + Sync>,
    ) -> Result<RecordingStarted, RecordingError>;

    fn stop(&mut self) -> Result<PcmRecording, RecordingError>;

    fn cancel(&mut self) -> Result<(), RecordingError>;
}

/// Reads and requests operating-system microphone authorization.
///
/// Implementations should trigger the native permission prompt only while the
/// status is `NotDetermined`; callers poll `authorization` for the final result.
pub trait MicrophonePermissionProvider: Send + Sync {
    fn authorization(&self) -> MicrophoneAuthorization;

    fn request_authorization(&self) -> MicrophoneAuthorization;
}

pub fn convert_interleaved_f32_to_pcm16(
    input: &[f32],
    source_sample_rate: u32,
    channels: u16,
    duration_ms: u64,
) -> Result<PcmRecording, RecordingError> {
    if source_sample_rate == 0 || channels == 0 {
        return Err(RecordingError::Capture(
            "audio format has a zero sample rate or channel count".to_owned(),
        ));
    }

    let mono = downmix_to_mono(input, usize::from(channels));
    let resampled = resample_linear(&mono, source_sample_rate, TARGET_SAMPLE_RATE);
    let samples = resampled.into_iter().map(f32_to_i16).collect();

    Ok(PcmRecording {
        samples,
        sample_rate: TARGET_SAMPLE_RATE,
        channels: 1,
        duration_ms,
    })
}

fn downmix_to_mono(input: &[f32], channels: usize) -> Vec<f32> {
    input
        .chunks_exact(channels)
        .map(|frame| frame.iter().copied().sum::<f32>() / channels as f32)
        .collect()
}

fn resample_linear(input: &[f32], source_rate: u32, target_rate: u32) -> Vec<f32> {
    if input.is_empty() || source_rate == target_rate {
        return input.to_vec();
    }

    let output_len = input.len().saturating_mul(target_rate as usize) / source_rate as usize;
    (0..output_len)
        .map(|index| {
            let source_position = index as f64 * source_rate as f64 / target_rate as f64;
            let lower = source_position.floor() as usize;
            let upper = lower.saturating_add(1).min(input.len() - 1);
            let fraction = (source_position - lower as f64) as f32;
            input[lower] + (input[upper] - input[lower]) * fraction
        })
        .collect()
}

fn f32_to_i16(sample: f32) -> i16 {
    let sample = sample.clamp(-1.0, 1.0);
    if sample == -1.0 {
        i16::MIN
    } else {
        (sample * f32::from(i16::MAX)).round() as i16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_stereo_to_mono_pcm16() {
        assert_eq!(
            Ok(PcmRecording {
                samples: vec![0, 16_384, i16::MIN],
                sample_rate: TARGET_SAMPLE_RATE,
                channels: 1,
                duration_ms: 25,
            }),
            convert_interleaved_f32_to_pcm16(
                &[1.0, -1.0, 0.5, 0.5, -1.0, -1.0],
                TARGET_SAMPLE_RATE,
                2,
                25,
            )
        );
    }

    #[test]
    fn resamples_48_khz_audio_to_16_khz() {
        let input = vec![0.25; 48_000];
        let Ok(recording) = convert_interleaved_f32_to_pcm16(&input, 48_000, 1, 1_000) else {
            panic!("valid audio should convert");
        };

        assert_eq!(16_000, recording.samples.len());
        assert!(recording.samples.iter().all(|sample| *sample == 8_192));
    }

    #[test]
    fn rejects_invalid_audio_format() {
        assert_eq!(
            Err(RecordingError::Capture(
                "audio format has a zero sample rate or channel count".to_owned()
            )),
            convert_interleaved_f32_to_pcm16(&[], 0, 1, 0)
        );
    }
}
