use std::{
    process::Command,
    ptr::NonNull,
    sync::mpsc,
    time::{Duration, Instant},
};

use block2::RcBlock;
use objc2::{AnyThread, MainThreadMarker, rc::autoreleasepool};
use objc2_avf_audio::{AVAudioCommonFormat, AVAudioFormat, AVAudioPCMBuffer};
use objc2_foundation::{NSError, NSLocale, NSOperationQueue, NSString};
use objc2_speech::{
    SFSpeechAudioBufferRecognitionRequest, SFSpeechRecognitionResult, SFSpeechRecognitionTask,
    SFSpeechRecognitionTaskState, SFSpeechRecognizer, SFSpeechRecognizerAuthorizationStatus,
};
use thiserror::Error;

mod production_adapter;
mod report;

use production_adapter::recognize_with_production_adapter;
pub use report::MacOsSpeechProbeReport;
use report::{
    AppleSpeechError, CancellationProbe, IntegrationFacts, LocaleCapability, ProbeHost,
    RecognitionProbe,
};

const SAMPLE_RATE: u32 = 16_000;
const STREAM_CHUNK_SAMPLES: usize = 1_600;
const RECOGNITION_TIMEOUT: Duration = Duration::from_secs(45);
const AUTHORIZATION_TIMEOUT: Duration = Duration::from_secs(120);
const CANCELLATION_OBSERVATION: Duration = Duration::from_secs(3);
const LONG_AUDIO_SECONDS: usize = 65;

#[derive(Debug, Error)]
pub enum MacOsSpeechProbeError {
    #[error("the probe audio must be non-empty 16 kHz mono PCM")]
    InvalidAudio,
    #[error("speech authorization did not complete within 120 seconds")]
    AuthorizationTimeout,
    #[error("the main thread marker is unavailable")]
    MainThreadUnavailable,
}

pub fn run_macos_speech_probe(
    standard_zh_samples: &[i16],
) -> Result<MacOsSpeechProbeReport, MacOsSpeechProbeError> {
    if standard_zh_samples.is_empty() {
        return Err(MacOsSpeechProbeError::InvalidAudio);
    }
    let Some(_main_thread) = MainThreadMarker::new() else {
        return Err(MacOsSpeechProbeError::MainThreadUnavailable);
    };
    autoreleasepool(|_| run_probe(standard_zh_samples))
}

fn run_probe(samples: &[i16]) -> Result<MacOsSpeechProbeReport, MacOsSpeechProbeError> {
    let authorization_before = unsafe { SFSpeechRecognizer::authorizationStatus() };
    let authorization_after = request_authorization(authorization_before)?;
    let locales = [None, Some("zh-CN"), Some("en-US")]
        .into_iter()
        .map(locale_capability)
        .collect();
    let authorized = authorization_after == SFSpeechRecognizerAuthorizationStatus::Authorized;

    let standard_system_managed =
        recognize_with_production_adapter(samples, false, authorized, "zh-CN");
    let second_system_managed_session =
        recognize_with_production_adapter(samples, false, authorized, "zh-CN");
    let standard_on_device = recognize_or_skip(samples, true, false, authorized, "zh-CN");
    let cancellation = cancel_or_skip(samples, authorized, "zh-CN");
    let long_samples: Vec<i16> = samples
        .iter()
        .copied()
        .cycle()
        .take(SAMPLE_RATE as usize * LONG_AUDIO_SECONDS)
        .collect();
    let long_audio_65_seconds =
        recognize_with_production_adapter(&long_samples, true, authorized, "zh-CN");

    Ok(MacOsSpeechProbeReport {
        schema_version: 2,
        host: ProbeHost {
            macos_version: command_output("/usr/bin/sw_vers", &["-productVersion"]),
            architecture: std::env::consts::ARCH.to_owned(),
        },
        authorization_before: authorization_name(authorization_before).to_owned(),
        authorization_after: authorization_name(authorization_after).to_owned(),
        locales,
        standard_system_managed,
        second_system_managed_session,
        standard_on_device,
        cancellation,
        long_audio_65_seconds,
        integration: IntegrationFacts {
            requires_saymore_api_key: false,
            requires_saymore_model_download: false,
            apple_service_may_use_network: true,
            apple_service_documents_throttling: true,
        },
    })
}

fn request_authorization(
    current: SFSpeechRecognizerAuthorizationStatus,
) -> Result<SFSpeechRecognizerAuthorizationStatus, MacOsSpeechProbeError> {
    if current != SFSpeechRecognizerAuthorizationStatus::NotDetermined {
        return Ok(current);
    }
    let (sender, receiver) = mpsc::sync_channel(1);
    let handler = RcBlock::new(move |status| {
        let _ = sender.send(status);
    });
    unsafe { SFSpeechRecognizer::requestAuthorization(&handler) };
    receiver
        .recv_timeout(AUTHORIZATION_TIMEOUT)
        .map_err(|_| MacOsSpeechProbeError::AuthorizationTimeout)
}

fn locale_capability(requested: Option<&str>) -> LocaleCapability {
    let label = requested.unwrap_or("system-default").to_owned();
    let Some(recognizer) = speech_recognizer(requested) else {
        return LocaleCapability {
            requested_locale: label,
            recognizer_created: false,
            resolved_locale: None,
            currently_available: false,
            supports_on_device_recognition: false,
        };
    };
    LocaleCapability {
        requested_locale: label,
        recognizer_created: true,
        resolved_locale: Some(
            unsafe { recognizer.locale() }
                .localeIdentifier()
                .to_string(),
        ),
        currently_available: unsafe { recognizer.isAvailable() },
        supports_on_device_recognition: unsafe { recognizer.supportsOnDeviceRecognition() },
    }
}

fn recognize_or_skip(
    samples: &[i16],
    on_device: bool,
    realtime_paced: bool,
    authorized: bool,
    locale: &str,
) -> RecognitionProbe {
    let mode = if on_device {
        "on-device"
    } else {
        "system-managed"
    };
    if !authorized {
        return skipped_recognition(
            samples,
            mode,
            locale,
            "speech recognition is not authorized",
        );
    }
    let Some(recognizer) = speech_recognizer(Some(locale)) else {
        return skipped_recognition(
            samples,
            mode,
            locale,
            "the locale recognizer is unavailable",
        );
    };
    if !unsafe { recognizer.isAvailable() } {
        return skipped_recognition(
            samples,
            mode,
            locale,
            "the recognizer is currently unavailable",
        );
    }
    if on_device && !unsafe { recognizer.supportsOnDeviceRecognition() } {
        return skipped_recognition(
            samples,
            mode,
            locale,
            "on-device recognition is unsupported",
        );
    }
    recognize(&recognizer, samples, on_device, realtime_paced, locale)
}

fn recognize(
    recognizer: &SFSpeechRecognizer,
    samples: &[i16],
    on_device: bool,
    realtime_paced: bool,
    locale: &str,
) -> RecognitionProbe {
    let started = Instant::now();
    let request = unsafe { SFSpeechAudioBufferRecognitionRequest::new() };
    unsafe {
        request.setShouldReportPartialResults(true);
        request.setRequiresOnDeviceRecognition(on_device);
        request.setAddsPunctuation(true);
    }
    let (sender, receiver) = mpsc::channel();
    let handler = result_handler(sender);
    let task = unsafe { recognizer.recognitionTaskWithRequest_resultHandler(&request, &handler) };
    let append_result = append_samples(&request, &task, samples, realtime_paced);
    unsafe { request.endAudio() };

    let mut partial_result_count = 0;
    let mut final_text = None;
    let mut error = append_result.as_ref().err().cloned();
    let finalization_started = Instant::now();
    while error.is_none()
        && final_text.is_none()
        && finalization_started.elapsed() < RECOGNITION_TIMEOUT
    {
        let remaining = RECOGNITION_TIMEOUT.saturating_sub(finalization_started.elapsed());
        match receiver.recv_timeout(remaining) {
            Ok(event) => {
                if let Some(text) = event.text {
                    partial_result_count += usize::from(!event.is_final);
                    if event.is_final {
                        final_text = Some(text);
                    }
                }
                if event.error.is_some() {
                    error = event.error;
                }
            }
            Err(_) => {
                error = Some(probe_error(
                    "SaymoreSpeechProbe",
                    -1,
                    "recognition timed out",
                ));
            }
        }
    }
    if final_text.is_none() && error.is_none() {
        error = Some(probe_error(
            "SaymoreSpeechProbe",
            -1,
            "recognition timed out",
        ));
    }
    unsafe { task.cancel() };
    RecognitionProbe {
        attempted: true,
        mode: if on_device {
            "on-device"
        } else {
            "system-managed"
        }
        .to_owned(),
        locale: locale.to_owned(),
        input_audio_seconds: audio_seconds(samples.len()),
        submitted_audio_seconds: append_result.map(audio_seconds).unwrap_or_default(),
        realtime_paced,
        elapsed_ms: started.elapsed().as_millis(),
        partial_result_count,
        final_text,
        error,
    }
}

fn cancel_or_skip(samples: &[i16], authorized: bool, locale: &str) -> CancellationProbe {
    if !authorized {
        return skipped_cancellation();
    }
    let Some(recognizer) = speech_recognizer(Some(locale)) else {
        return skipped_cancellation();
    };
    if !unsafe { recognizer.isAvailable() } {
        return skipped_cancellation();
    }
    let request = unsafe { SFSpeechAudioBufferRecognitionRequest::new() };
    unsafe { request.setShouldReportPartialResults(true) };
    let (sender, receiver) = mpsc::channel();
    let handler = result_handler(sender);
    let task = unsafe { recognizer.recognitionTaskWithRequest_resultHandler(&request, &handler) };
    let append_error = append_samples(&request, &task, samples, false).err();
    unsafe { task.cancel() };
    let event = receiver.recv_timeout(CANCELLATION_OBSERVATION).ok();
    CancellationProbe {
        attempted: true,
        task_reported_cancelled: unsafe { task.isCancelled() },
        callback_received_after_cancel: event.is_some(),
        final_result_received_after_cancel: event.as_ref().is_some_and(|event| event.is_final),
        callback_error: append_error.or_else(|| event.and_then(|event| event.error)),
    }
}

fn speech_recognizer(locale: Option<&str>) -> Option<objc2::rc::Retained<SFSpeechRecognizer>> {
    match locale {
        Some(identifier) => {
            let identifier = NSString::from_str(identifier);
            let locale = NSLocale::initWithLocaleIdentifier(NSLocale::alloc(), &identifier);
            unsafe { SFSpeechRecognizer::initWithLocale(SFSpeechRecognizer::alloc(), &locale) }
        }
        None => unsafe { SFSpeechRecognizer::init(SFSpeechRecognizer::alloc()) },
    }
    .inspect(|recognizer| {
        let queue = NSOperationQueue::new();
        queue.setMaxConcurrentOperationCount(1);
        unsafe { recognizer.setQueue(&queue) };
    })
}

fn append_samples(
    request: &SFSpeechAudioBufferRecognitionRequest,
    task: &SFSpeechRecognitionTask,
    samples: &[i16],
    realtime_paced: bool,
) -> Result<usize, AppleSpeechError> {
    let mut submitted_samples = 0;
    for chunk in samples.chunks(STREAM_CHUNK_SAMPLES) {
        if unsafe { task.state() } == SFSpeechRecognitionTaskState::Completed {
            break;
        }
        let frame_count = u32::try_from(chunk.len())
            .map_err(|error| probe_error("SaymoreSpeechProbe", -2, &error.to_string()))?;
        let format = unsafe {
            AVAudioFormat::initWithCommonFormat_sampleRate_channels_interleaved(
                AVAudioFormat::alloc(),
                AVAudioCommonFormat::PCMFormatInt16,
                f64::from(SAMPLE_RATE),
                1,
                false,
            )
        }
        .ok_or_else(|| probe_error("SaymoreSpeechProbe", -2, "could not create PCM format"))?;
        let buffer = unsafe {
            AVAudioPCMBuffer::initWithPCMFormat_frameCapacity(
                AVAudioPCMBuffer::alloc(),
                &format,
                frame_count,
            )
        }
        .ok_or_else(|| probe_error("SaymoreSpeechProbe", -2, "could not create PCM buffer"))?;
        let channels = unsafe { buffer.int16ChannelData() };
        let Some(channel) = NonNull::new(channels).map(|channels| unsafe { *channels.as_ptr() })
        else {
            return Err(probe_error(
                "SaymoreSpeechProbe",
                -2,
                "PCM buffer has no 16-bit channel data",
            ));
        };
        unsafe {
            std::ptr::copy_nonoverlapping(chunk.as_ptr(), channel.as_ptr(), chunk.len());
            buffer.setFrameLength(frame_count);
            request.appendAudioPCMBuffer(&buffer);
        }
        submitted_samples += chunk.len();
        if realtime_paced {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    Ok(submitted_samples)
}

struct RecognitionEvent {
    text: Option<String>,
    is_final: bool,
    error: Option<AppleSpeechError>,
}

fn result_handler(
    sender: mpsc::Sender<RecognitionEvent>,
) -> RcBlock<dyn Fn(*mut SFSpeechRecognitionResult, *mut NSError)> {
    RcBlock::new(
        move |result: *mut SFSpeechRecognitionResult, error: *mut NSError| {
            let result = unsafe { result.as_ref() };
            let error = unsafe { error.as_ref() };
            let event = RecognitionEvent {
                text: result.map(|result| unsafe {
                    result.bestTranscription().formattedString().to_string()
                }),
                is_final: result.is_some_and(|result| unsafe { result.isFinal() }),
                error: error.map(apple_error),
            };
            let _ = sender.send(event);
        },
    )
}

fn apple_error(error: &NSError) -> AppleSpeechError {
    AppleSpeechError {
        domain: error.domain().to_string(),
        code: error.code(),
        message: error.localizedDescription().to_string(),
    }
}

fn skipped_recognition(
    samples: &[i16],
    mode: &str,
    locale: &str,
    message: &str,
) -> RecognitionProbe {
    RecognitionProbe {
        attempted: false,
        mode: mode.to_owned(),
        locale: locale.to_owned(),
        input_audio_seconds: audio_seconds(samples.len()),
        submitted_audio_seconds: 0.0,
        realtime_paced: false,
        elapsed_ms: 0,
        partial_result_count: 0,
        final_text: None,
        error: Some(probe_error("SaymoreSpeechProbe", -3, message)),
    }
}

fn skipped_cancellation() -> CancellationProbe {
    CancellationProbe {
        attempted: false,
        task_reported_cancelled: false,
        callback_received_after_cancel: false,
        final_result_received_after_cancel: false,
        callback_error: None,
    }
}

fn probe_error(domain: &str, code: isize, message: &str) -> AppleSpeechError {
    AppleSpeechError {
        domain: domain.to_owned(),
        code,
        message: message.to_owned(),
    }
}

fn audio_seconds(sample_count: usize) -> f64 {
    sample_count as f64 / f64::from(SAMPLE_RATE)
}

fn authorization_name(status: SFSpeechRecognizerAuthorizationStatus) -> &'static str {
    match status {
        SFSpeechRecognizerAuthorizationStatus::NotDetermined => "not-determined",
        SFSpeechRecognizerAuthorizationStatus::Denied => "denied",
        SFSpeechRecognizerAuthorizationStatus::Restricted => "restricted",
        SFSpeechRecognizerAuthorizationStatus::Authorized => "authorized",
        _ => "unknown",
    }
}

fn command_output(program: &str, args: &[&str]) -> String {
    Command::new(program)
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_else(|| "unknown".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_pcm_duration_in_seconds() {
        assert_eq!(3.0, audio_seconds(SAMPLE_RATE as usize * 3));
    }

    #[test]
    fn labels_known_authorization_states() {
        assert_eq!(
            "authorized",
            authorization_name(SFSpeechRecognizerAuthorizationStatus::Authorized)
        );
        assert_eq!(
            "denied",
            authorization_name(SFSpeechRecognizerAuthorizationStatus::Denied)
        );
    }
}
