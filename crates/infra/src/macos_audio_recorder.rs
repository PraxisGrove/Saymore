use std::{
    mem,
    sync::{Arc, Mutex, mpsc},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use cpal::{
    SampleFormat, Stream,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use template_app::{
    AudioRecorder, MicrophoneAuthorization, MicrophonePermissionProvider, PcmChunk, PcmRecording,
    RecordingError, RecordingMetrics, RecordingStarted, convert_interleaved_f32_to_pcm16,
};

use crate::MacOsMicrophonePermission;

const METRICS_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Default)]
pub struct MacOsAudioRecorder {
    active: Option<ActiveRecording>,
}

impl MacOsAudioRecorder {
    pub fn is_recording(&self) -> bool {
        self.active.is_some()
    }
}

struct ActiveRecording {
    stream: Stream,
    capture: Arc<Mutex<CaptureBuffer>>,
    source_sample_rate: u32,
    stream_tx: mpsc::SyncSender<Vec<f32>>,
    worker: JoinHandle<()>,
}

struct CaptureBuffer {
    samples: Vec<f32>,
    started: Instant,
    last_metrics: Instant,
    peak_since_metrics: f32,
    streaming_pending: Vec<f32>,
    error: Option<String>,
}

impl CaptureBuffer {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            samples: Vec::new(),
            started: now,
            last_metrics: now,
            peak_since_metrics: 0.0,
            streaming_pending: Vec::new(),
            error: None,
        }
    }
}

impl AudioRecorder for MacOsAudioRecorder {
    fn start(
        &mut self,
        on_metrics: Arc<dyn Fn(RecordingMetrics) + Send + Sync>,
        on_audio_chunk: Arc<dyn Fn(PcmChunk) + Send + Sync>,
    ) -> Result<RecordingStarted, RecordingError> {
        if self.active.is_some() {
            return Err(RecordingError::AlreadyRecording);
        }
        if MacOsMicrophonePermission.authorization() != MicrophoneAuthorization::Granted {
            return Err(RecordingError::PermissionDenied);
        }

        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or(RecordingError::NoInputDevice)?;
        let input_device_name = device.to_string();
        let supported = device
            .default_input_config()
            .map_err(|error| RecordingError::Capture(error.to_string()))?;
        if supported.sample_format() != SampleFormat::F32 {
            return Err(RecordingError::UnsupportedSampleFormat(
                supported.sample_format().to_string(),
            ));
        }

        let source_sample_rate = supported.sample_rate();
        let channels = usize::from(supported.channels());
        let config = supported.config();
        let capture = Arc::new(Mutex::new(CaptureBuffer::new()));
        let (stream_tx, stream_rx) = mpsc::sync_channel::<Vec<f32>>(16);
        let worker = thread::Builder::new()
            .name("saymore-audio-stream".to_owned())
            .spawn(move || {
                for mono in stream_rx {
                    if let Ok(chunk) =
                        convert_interleaved_f32_to_pcm16(&mono, source_sample_rate, 1, 0)
                        && !chunk.samples.is_empty()
                    {
                        on_audio_chunk(PcmChunk {
                            samples: chunk.samples,
                            sample_rate: chunk.sample_rate,
                        });
                    }
                }
            })
            .map_err(|error| RecordingError::Capture(error.to_string()))?;
        let data_capture = Arc::clone(&capture);
        let error_capture = Arc::clone(&capture);
        let callback_stream_tx = stream_tx.clone();
        let stream = device
            .build_input_stream(
                config,
                move |data: &[f32], _| {
                    capture_input(
                        data,
                        channels,
                        source_sample_rate,
                        &data_capture,
                        &on_metrics,
                        &callback_stream_tx,
                    );
                },
                move |error| {
                    if let Ok(mut capture) = error_capture.lock() {
                        capture.error = Some(error.to_string());
                    }
                },
                None,
            )
            .map_err(|error| RecordingError::Capture(error.to_string()))?;
        stream
            .play()
            .map_err(|error| RecordingError::Capture(error.to_string()))?;
        self.active = Some(ActiveRecording {
            stream,
            capture,
            source_sample_rate,
            stream_tx,
            worker,
        });
        Ok(RecordingStarted { input_device_name })
    }

    fn stop(&mut self) -> Result<PcmRecording, RecordingError> {
        let active = self.active.take().ok_or(RecordingError::NotRecording)?;
        drop(active.stream);

        let mut capture = active
            .capture
            .lock()
            .map_err(|_| RecordingError::Capture("audio buffer lock was poisoned".to_owned()))?;
        if let Some(error) = capture.error.take() {
            return Err(RecordingError::Capture(error));
        }
        let duration_ms = elapsed_ms(capture.started.elapsed());
        let samples = mem::take(&mut capture.samples);
        let pending = mem::take(&mut capture.streaming_pending);
        drop(capture);
        if !pending.is_empty() {
            active
                .stream_tx
                .send(pending)
                .map_err(|_| RecordingError::Capture("audio stream worker stopped".to_owned()))?;
        }
        drop(active.stream_tx);
        active
            .worker
            .join()
            .map_err(|_| RecordingError::Capture("audio stream worker panicked".to_owned()))?;
        convert_interleaved_f32_to_pcm16(&samples, active.source_sample_rate, 1, duration_ms)
    }

    fn cancel(&mut self) -> Result<(), RecordingError> {
        let active = self.active.take().ok_or(RecordingError::NotRecording)?;
        drop(active.stream);
        drop(active.stream_tx);
        drop(active.worker);
        Ok(())
    }
}

fn capture_input(
    data: &[f32],
    channels: usize,
    source_sample_rate: u32,
    capture: &Mutex<CaptureBuffer>,
    on_metrics: &Arc<dyn Fn(RecordingMetrics) + Send + Sync>,
    stream_tx: &mpsc::SyncSender<Vec<f32>>,
) {
    if channels == 0 {
        return;
    }

    let mut stream_batch = None;
    let metrics = if let Ok(mut capture) = capture.lock() {
        let mut level = 0.0_f32;
        for frame in data.chunks_exact(channels) {
            let sample = frame.iter().copied().sum::<f32>() / channels as f32;
            level = level.max(sample.abs());
            capture.samples.push(sample);
            capture.streaming_pending.push(sample);
        }
        let stream_batch_len = source_sample_rate as usize / 10;
        if capture.streaming_pending.len() >= stream_batch_len {
            let remaining = capture.streaming_pending.split_off(stream_batch_len);
            stream_batch = Some(mem::replace(&mut capture.streaming_pending, remaining));
        }
        capture.peak_since_metrics = capture.peak_since_metrics.max(level);

        let now = Instant::now();
        if now.duration_since(capture.last_metrics) >= METRICS_INTERVAL {
            capture.last_metrics = now;
            let level = normalize_peak_level(capture.peak_since_metrics);
            capture.peak_since_metrics = 0.0;
            Some(RecordingMetrics {
                elapsed_ms: elapsed_ms(now.duration_since(capture.started)),
                input_sample_count: capture.samples.len(),
                level,
            })
        } else {
            None
        }
    } else {
        None
    };

    if let Some(metrics) = metrics {
        on_metrics(metrics);
    }
    if let Some(batch) = stream_batch
        && let Err(error) = stream_tx.try_send(batch)
        && let Ok(mut capture) = capture.lock()
    {
        capture.error = Some(match error {
            mpsc::TrySendError::Full(_) => "audio stream queue is full".to_owned(),
            mpsc::TrySendError::Disconnected(_) => "audio stream worker stopped".to_owned(),
        });
    }
}

fn normalize_peak_level(peak: f32) -> f32 {
    const SILENCE_GATE: f32 = 0.015;
    if peak < SILENCE_GATE {
        return 0.0;
    }
    ((20.0 * peak.clamp(0.0, 1.0).log10() + 50.0) / 50.0).clamp(0.0, 1.0)
}

fn elapsed_ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).map_or(u64::MAX, |milliseconds| milliseconds)
}

#[cfg(test)]
mod tests {
    use super::normalize_peak_level;

    #[test]
    fn normalizes_peak_level_for_visible_meter_motion() {
        assert_eq!(0.0, normalize_peak_level(0.0));
        assert_eq!(0.0, normalize_peak_level(0.01));
        assert!(normalize_peak_level(0.02) > 0.0);
        assert!(normalize_peak_level(0.1) > normalize_peak_level(0.02));
        assert_eq!(1.0, normalize_peak_level(1.0));
    }
}
