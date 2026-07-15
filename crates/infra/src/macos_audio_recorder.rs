use std::{
    mem,
    str::FromStr,
    sync::{Arc, Mutex, mpsc},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use cpal::{
    Device, DeviceId, FromSample, I24, Sample, SampleFormat, SizedSample, Stream, StreamConfig,
    U24,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use template_app::{
    AudioInputDevice, AudioRecorder, MicrophoneAuthorization, MicrophonePermissionProvider,
    PcmChunk, PcmRecording, RecordingError, RecordingMetrics, RecordingStarted,
    convert_interleaved_f32_to_pcm16,
};

use crate::MacOsMicrophonePermission;

const METRICS_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Default)]
pub struct MacOsAudioRecorder {
    active: Option<ActiveRecording>,
    prepared: Option<PreparedInput>,
    preferred_input_device_id: Option<String>,
}

impl MacOsAudioRecorder {
    pub fn with_preferred_input_device_id(preferred_input_device_id: Option<String>) -> Self {
        Self {
            active: None,
            prepared: None,
            preferred_input_device_id,
        }
    }

    pub fn is_recording(&self) -> bool {
        self.active.is_some()
    }

    pub fn set_preferred_input_device_id(&mut self, preferred_input_device_id: Option<String>) {
        if self.preferred_input_device_id == preferred_input_device_id {
            return;
        }
        self.preferred_input_device_id = preferred_input_device_id;
        if self.active.is_none() {
            self.prepared = None;
        }
    }

    pub fn prepare(&mut self) -> Result<(), RecordingError> {
        if self.active.is_some() {
            return Ok(());
        }
        if MacOsMicrophonePermission.authorization() != MicrophoneAuthorization::Granted {
            return Err(RecordingError::PermissionDenied);
        }
        let started = Instant::now();
        let (device, _) = self.selected_input_device()?;
        let (_, build_ms) = self.ensure_prepared_input(&device)?;
        tracing::info!(
            target: "saymore::diagnostics",
            event = "recording.audio_prepared",
            build_ms,
            total_ms = started.elapsed().as_millis()
        );
        Ok(())
    }

    pub fn input_devices() -> Result<Vec<AudioInputDevice>, RecordingError> {
        let host = cpal::default_host();
        let default_id = host
            .default_input_device()
            .and_then(|device| device.id().ok());
        let mut devices = host
            .input_devices()
            .map_err(|error| RecordingError::Capture(error.to_string()))?
            .filter_map(|device| {
                let id = device.id().ok()?;
                Some(AudioInputDevice {
                    id: id.to_string(),
                    name: device.to_string(),
                    is_system_default: default_id.as_ref() == Some(&id),
                })
            })
            .collect::<Vec<_>>();
        devices.sort_by(|left, right| {
            right
                .is_system_default
                .cmp(&left.is_system_default)
                .then_with(|| left.name.cmp(&right.name))
        });
        Ok(devices)
    }

    fn selected_input_device(&self) -> Result<(Device, bool), RecordingError> {
        let host = cpal::default_host();
        if let Some(preferred_id) = &self.preferred_input_device_id {
            let selected = DeviceId::from_str(preferred_id)
                .ok()
                .and_then(|id| host.device_by_id(&id));
            if let Some(device) = selected {
                return Ok((device, false));
            }

            let fallback = host
                .default_input_device()
                .ok_or(RecordingError::NoInputDevice)?;
            return Ok((fallback, true));
        }

        host.default_input_device()
            .map(|device| (device, false))
            .ok_or(RecordingError::NoInputDevice)
    }

    fn ensure_prepared_input(
        &mut self,
        device: &Device,
    ) -> Result<(&PreparedInput, u128), RecordingError> {
        let device_id = device
            .id()
            .map_err(|error| RecordingError::Capture(error.to_string()))?
            .to_string();
        if self
            .prepared
            .as_ref()
            .is_some_and(|input| input.device_id == device_id)
        {
            return self
                .prepared
                .as_ref()
                .map(|input| (input, 0))
                .ok_or_else(|| RecordingError::Capture("prepared input disappeared".to_owned()));
        }

        let supported = device
            .default_input_config()
            .map_err(|error| RecordingError::Capture(error.to_string()))?;
        let source_sample_rate = supported.sample_rate();
        let channels = usize::from(supported.channels());
        let session = Arc::new(Mutex::new(None));
        let build_started = Instant::now();
        let stream = build_stream_for_format(
            device,
            supported.sample_format(),
            supported.config(),
            channels,
            source_sample_rate,
            &session,
        )?;
        let build_ms = build_started.elapsed().as_millis();
        self.prepared = Some(PreparedInput {
            device_id,
            stream,
            source_sample_rate,
            session,
        });
        self.prepared
            .as_ref()
            .map(|input| (input, build_ms))
            .ok_or_else(|| RecordingError::Capture("failed to retain prepared input".to_owned()))
    }
}

struct ActiveRecording {
    capture: Arc<Mutex<CaptureBuffer>>,
    source_sample_rate: u32,
    stream_tx: mpsc::SyncSender<Vec<f32>>,
    worker: JoinHandle<()>,
}

struct PreparedInput {
    device_id: String,
    stream: Stream,
    source_sample_rate: u32,
    session: Arc<Mutex<Option<StreamSession>>>,
}

#[derive(Clone)]
struct StreamSession {
    capture: Arc<Mutex<CaptureBuffer>>,
    on_metrics: Arc<dyn Fn(RecordingMetrics) + Send + Sync>,
    stream_tx: mpsc::SyncSender<Vec<f32>>,
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
        let startup_started = Instant::now();
        if self.active.is_some() {
            return Err(RecordingError::AlreadyRecording);
        }
        if MacOsMicrophonePermission.authorization() != MicrophoneAuthorization::Granted {
            return Err(RecordingError::PermissionDenied);
        }

        let device_started = Instant::now();
        let (device, used_system_fallback) = self.selected_input_device()?;
        let input_device_name = device.to_string();
        let device_ms = device_started.elapsed().as_millis();
        let config_started = Instant::now();
        let (prepared, build_ms) = self.ensure_prepared_input(&device)?;
        let source_sample_rate = prepared.source_sample_rate;
        let capture = Arc::new(Mutex::new(CaptureBuffer::new()));
        let (stream_tx, stream_rx) = mpsc::sync_channel::<Vec<f32>>(16);
        let config_ms = config_started.elapsed().as_millis();
        let worker_started = Instant::now();
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
        let worker_ms = worker_started.elapsed().as_millis();
        let prepared = self
            .prepared
            .as_mut()
            .ok_or_else(|| RecordingError::Capture("prepared input disappeared".to_owned()))?;
        let mut session = prepared
            .session
            .lock()
            .map_err(|_| RecordingError::Capture("audio session lock was poisoned".to_owned()))?;
        *session = Some(StreamSession {
            capture: Arc::clone(&capture),
            on_metrics,
            stream_tx: stream_tx.clone(),
        });
        drop(session);
        let play_started = Instant::now();
        if let Err(error) = prepared.stream.play() {
            if let Ok(mut session) = prepared.session.lock() {
                *session = None;
            }
            return Err(RecordingError::Capture(error.to_string()));
        }
        let play_ms = play_started.elapsed().as_millis();
        self.active = Some(ActiveRecording {
            capture,
            source_sample_rate,
            stream_tx,
            worker,
        });
        tracing::info!(
            target: "saymore::diagnostics",
            event = "recording.audio_startup",
            device_ms,
            config_ms,
            worker_ms,
            build_ms,
            play_ms,
            total_ms = startup_started.elapsed().as_millis()
        );
        Ok(RecordingStarted {
            input_device_name,
            used_system_fallback,
        })
    }

    fn stop(&mut self) -> Result<PcmRecording, RecordingError> {
        let active = self.active.take().ok_or(RecordingError::NotRecording)?;
        self.pause_prepared_input()?;

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
        self.pause_prepared_input()?;
        drop(active.stream_tx);
        drop(active.worker);
        Ok(())
    }
}

impl MacOsAudioRecorder {
    fn pause_prepared_input(&mut self) -> Result<(), RecordingError> {
        let prepared = self
            .prepared
            .as_mut()
            .ok_or_else(|| RecordingError::Capture("prepared input disappeared".to_owned()))?;
        let pause_result = prepared
            .stream
            .pause()
            .map_err(|error| RecordingError::Capture(error.to_string()));
        let mut session = prepared
            .session
            .lock()
            .map_err(|_| RecordingError::Capture("audio session lock was poisoned".to_owned()))?;
        *session = None;
        pause_result
    }
}

#[allow(clippy::too_many_arguments)]
fn build_stream_for_format(
    device: &Device,
    sample_format: SampleFormat,
    config: StreamConfig,
    channels: usize,
    source_sample_rate: u32,
    session: &Arc<Mutex<Option<StreamSession>>>,
) -> Result<Stream, RecordingError> {
    match sample_format {
        SampleFormat::I8 => {
            build_input_stream::<i8>(device, config, channels, source_sample_rate, session)
        }
        SampleFormat::I16 => {
            build_input_stream::<i16>(device, config, channels, source_sample_rate, session)
        }
        SampleFormat::I24 => {
            build_input_stream::<I24>(device, config, channels, source_sample_rate, session)
        }
        SampleFormat::I32 => {
            build_input_stream::<i32>(device, config, channels, source_sample_rate, session)
        }
        SampleFormat::I64 => {
            build_input_stream::<i64>(device, config, channels, source_sample_rate, session)
        }
        SampleFormat::U8 => {
            build_input_stream::<u8>(device, config, channels, source_sample_rate, session)
        }
        SampleFormat::U16 => {
            build_input_stream::<u16>(device, config, channels, source_sample_rate, session)
        }
        SampleFormat::U24 => {
            build_input_stream::<U24>(device, config, channels, source_sample_rate, session)
        }
        SampleFormat::U32 => {
            build_input_stream::<u32>(device, config, channels, source_sample_rate, session)
        }
        SampleFormat::U64 => {
            build_input_stream::<u64>(device, config, channels, source_sample_rate, session)
        }
        SampleFormat::F32 => {
            build_input_stream::<f32>(device, config, channels, source_sample_rate, session)
        }
        SampleFormat::F64 => {
            build_input_stream::<f64>(device, config, channels, source_sample_rate, session)
        }
        SampleFormat::DsdU8 | SampleFormat::DsdU16 | SampleFormat::DsdU32 | _ => Err(
            RecordingError::UnsupportedSampleFormat(sample_format.to_string()),
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn build_input_stream<T>(
    device: &Device,
    config: StreamConfig,
    channels: usize,
    source_sample_rate: u32,
    session: &Arc<Mutex<Option<StreamSession>>>,
) -> Result<Stream, RecordingError>
where
    T: Sample + SizedSample,
    f32: FromSample<T>,
{
    let data_session = Arc::clone(session);
    let error_session = Arc::clone(session);
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                let session = data_session.lock().ok().and_then(|session| session.clone());
                if let Some(session) = session {
                    capture_input(
                        data,
                        channels,
                        source_sample_rate,
                        &session.capture,
                        &session.on_metrics,
                        &session.stream_tx,
                    );
                }
            },
            move |error| {
                let capture = error_session.lock().ok().and_then(|session| {
                    session.as_ref().map(|session| Arc::clone(&session.capture))
                });
                if let Some(capture) = capture
                    && let Ok(mut capture) = capture.lock()
                {
                    capture.error = Some(error.to_string());
                }
            },
            None,
        )
        .map_err(|error| RecordingError::Capture(error.to_string()))
}

fn capture_input<T>(
    data: &[T],
    channels: usize,
    source_sample_rate: u32,
    capture: &Mutex<CaptureBuffer>,
    on_metrics: &Arc<dyn Fn(RecordingMetrics) + Send + Sync>,
    stream_tx: &mpsc::SyncSender<Vec<f32>>,
) where
    T: Sample,
    f32: FromSample<T>,
{
    if channels == 0 {
        return;
    }

    let mut stream_batch = None;
    let metrics = if let Ok(mut capture) = capture.lock() {
        let mut level = 0.0_f32;
        for frame in data.chunks_exact(channels) {
            let sample = frame
                .iter()
                .copied()
                .map(|sample| sample.to_sample::<f32>())
                .sum::<f32>()
                / channels as f32;
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
