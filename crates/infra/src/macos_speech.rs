use std::{
    io,
    process::Command,
    ptr::NonNull,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use block2::RcBlock;
use objc2::{AnyThread, rc::autoreleasepool};
use objc2_avf_audio::{AVAudioCommonFormat, AVAudioFormat, AVAudioPCMBuffer};
use objc2_foundation::{NSArray, NSError, NSLocale, NSOperationQueue, NSString};
use objc2_speech::{
    SFSpeechAudioBufferRecognitionRequest, SFSpeechRecognitionResult, SFSpeechRecognitionTask,
    SFSpeechRecognizer, SFSpeechRecognizerAuthorizationStatus,
};
use template_app::{
    SpeechRecognitionError, SpeechRecognitionHints, StreamingRecognitionSession,
    StreamingSpeechRecognizer,
};

const SAMPLE_RATE: u32 = 16_000;
const MAX_CONTEXTUAL_STRINGS: usize = 100;
const AUTHORIZATION_TIMEOUT: Duration = Duration::from_secs(120);
const START_TIMEOUT: Duration = Duration::from_secs(125);
const FINAL_TIMEOUT: Duration = Duration::from_secs(45);

/// Streams Saymore's 16 kHz mono PCM into Apple's system Speech recognizer.
///
/// The recognizer uses the system-managed recognition mode. Apple may process
/// audio on-device or use its service depending on the selected locale and host.
#[derive(Debug, Clone, Default)]
pub struct MacOsSpeechRecognizer {
    locale: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacOsSpeechAuthorization {
    NotDetermined,
    Denied,
    Restricted,
    Authorized,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacOsSpeechCapability {
    pub authorization: MacOsSpeechAuthorization,
    pub available: bool,
}

pub fn macos_speech_capability() -> MacOsSpeechCapability {
    let authorization = speech_authorization();
    let available =
        speech_recognizer(None).is_some_and(|recognizer| unsafe { recognizer.isAvailable() });
    MacOsSpeechCapability {
        authorization,
        available,
    }
}

pub fn request_macos_speech_authorization(
    on_complete: Arc<dyn Fn(MacOsSpeechAuthorization) + Send + Sync>,
) {
    let current = speech_authorization();
    if current != MacOsSpeechAuthorization::NotDetermined {
        on_complete(current);
        return;
    }
    let handler = RcBlock::new(move |status| {
        on_complete(map_authorization(status));
    });
    unsafe { SFSpeechRecognizer::requestAuthorization(&handler) };
}

pub fn open_speech_recognition_privacy_settings() -> Result<(), io::Error> {
    let status = Command::new("/usr/bin/open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_SpeechRecognition")
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "System Settings exited with status {status}"
        )))
    }
}

pub fn macos_product_version() -> Result<String, io::Error> {
    let output = Command::new("/usr/bin/sw_vers")
        .arg("-productVersion")
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "sw_vers exited with status {}",
            output.status
        )));
    }
    let version = String::from_utf8(output.stdout)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let version = version.trim();
    if version.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "sw_vers returned an empty product version",
        ));
    }
    Ok(version.to_owned())
}

impl MacOsSpeechRecognizer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Selects a locale explicitly instead of using the system dictation locale.
    pub fn for_locale(locale: impl Into<String>) -> Result<Self, SpeechRecognitionError> {
        let locale = locale.into();
        if locale.trim().is_empty() {
            return Err(SpeechRecognitionError::Protocol(
                "Apple Speech locale must not be empty".to_owned(),
            ));
        }
        Ok(Self {
            locale: Some(locale),
        })
    }
}

impl StreamingSpeechRecognizer for MacOsSpeechRecognizer {
    fn start(
        &self,
        hints: SpeechRecognitionHints,
        on_partial: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Result<Box<dyn StreamingRecognitionSession>, SpeechRecognitionError> {
        let (event_tx, event_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let (result_tx, result_rx) = mpsc::sync_channel(1);
        let locale = self.locale.clone();
        let callback_event_tx = event_tx.clone();
        thread::Builder::new()
            .name("saymore-apple-speech".to_owned())
            .spawn(move || {
                autoreleasepool(|_| {
                    run_worker(
                        locale,
                        hints,
                        on_partial,
                        callback_event_tx,
                        event_rx,
                        ready_tx,
                        result_tx,
                    )
                });
            })
            .map_err(transport_error)?;

        ready_rx
            .recv_timeout(START_TIMEOUT)
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => SpeechRecognitionError::Timeout,
                mpsc::RecvTimeoutError::Disconnected => {
                    SpeechRecognitionError::Transport("Apple Speech worker stopped".to_owned())
                }
            })??;

        Ok(Box::new(MacOsSpeechSession {
            event_tx,
            result_rx,
            closed: AtomicBool::new(false),
        }))
    }
}

enum WorkerEvent {
    Audio(Vec<i16>),
    Finish,
    Cancel,
    Recognition(RecognitionEvent),
}

struct RecognitionEvent {
    text: Option<String>,
    is_final: bool,
    error: Option<SpeechRecognitionError>,
}

fn run_worker(
    locale: Option<String>,
    hints: SpeechRecognitionHints,
    on_partial: Arc<dyn Fn(String) + Send + Sync>,
    event_tx: mpsc::Sender<WorkerEvent>,
    event_rx: mpsc::Receiver<WorkerEvent>,
    ready_tx: mpsc::SyncSender<Result<(), SpeechRecognitionError>>,
    result_tx: mpsc::SyncSender<Result<String, SpeechRecognitionError>>,
) {
    let initialized = initialize_recognition(locale.as_deref(), &hints, event_tx);
    let InitializedRecognition {
        _recognizer,
        request,
        task,
        _handler,
    } = match initialized {
        Ok(initialized) => initialized,
        Err(error) => {
            let _ = ready_tx.send(Err(error));
            return;
        }
    };
    if ready_tx.send(Ok(())).is_err() {
        unsafe { task.cancel() };
        return;
    }
    let result = process_events(&request, &event_rx, on_partial);
    unsafe { task.cancel() };
    if let Some(result) = result {
        let _ = result_tx.send(result);
    }
}

type AppleResultHandler = RcBlock<dyn Fn(*mut SFSpeechRecognitionResult, *mut NSError)>;

struct InitializedRecognition {
    _recognizer: objc2::rc::Retained<SFSpeechRecognizer>,
    request: objc2::rc::Retained<SFSpeechAudioBufferRecognitionRequest>,
    task: objc2::rc::Retained<SFSpeechRecognitionTask>,
    _handler: AppleResultHandler,
}

fn initialize_recognition(
    locale: Option<&str>,
    hints: &SpeechRecognitionHints,
    event_tx: mpsc::Sender<WorkerEvent>,
) -> Result<InitializedRecognition, SpeechRecognitionError> {
    authorize_speech_recognition()?;
    let recognizer = speech_recognizer(locale).ok_or_else(|| {
        SpeechRecognitionError::Protocol("Apple Speech locale is unsupported".to_owned())
    })?;
    if !unsafe { recognizer.isAvailable() } {
        return Err(SpeechRecognitionError::Transport(
            "Apple Speech is currently unavailable".to_owned(),
        ));
    }
    let request = unsafe { SFSpeechAudioBufferRecognitionRequest::new() };
    configure_request(&request, hints);
    let handler = result_handler(event_tx);
    let task = unsafe { recognizer.recognitionTaskWithRequest_resultHandler(&request, &handler) };
    Ok(InitializedRecognition {
        _recognizer: recognizer,
        request,
        task,
        _handler: handler,
    })
}

fn authorize_speech_recognition() -> Result<(), SpeechRecognitionError> {
    let current = unsafe { SFSpeechRecognizer::authorizationStatus() };
    if current != SFSpeechRecognizerAuthorizationStatus::NotDetermined {
        return authorization_result(current);
    }
    let (sender, receiver) = mpsc::sync_channel(1);
    let handler = RcBlock::new(move |status| {
        let _ = sender.send(status);
    });
    unsafe { SFSpeechRecognizer::requestAuthorization(&handler) };
    receiver
        .recv_timeout(AUTHORIZATION_TIMEOUT)
        .map_err(|_| SpeechRecognitionError::Timeout)
        .and_then(authorization_result)
}

fn authorization_result(
    status: SFSpeechRecognizerAuthorizationStatus,
) -> Result<(), SpeechRecognitionError> {
    match status {
        SFSpeechRecognizerAuthorizationStatus::Authorized => Ok(()),
        SFSpeechRecognizerAuthorizationStatus::Denied
        | SFSpeechRecognizerAuthorizationStatus::Restricted => {
            Err(SpeechRecognitionError::Authentication)
        }
        SFSpeechRecognizerAuthorizationStatus::NotDetermined => {
            Err(SpeechRecognitionError::Protocol(
                "Apple Speech authorization is undetermined".to_owned(),
            ))
        }
        _ => Err(SpeechRecognitionError::Protocol(
            "Apple Speech returned an unknown authorization status".to_owned(),
        )),
    }
}

fn speech_authorization() -> MacOsSpeechAuthorization {
    map_authorization(unsafe { SFSpeechRecognizer::authorizationStatus() })
}

fn map_authorization(status: SFSpeechRecognizerAuthorizationStatus) -> MacOsSpeechAuthorization {
    match status {
        SFSpeechRecognizerAuthorizationStatus::NotDetermined => {
            MacOsSpeechAuthorization::NotDetermined
        }
        SFSpeechRecognizerAuthorizationStatus::Denied => MacOsSpeechAuthorization::Denied,
        SFSpeechRecognizerAuthorizationStatus::Restricted => MacOsSpeechAuthorization::Restricted,
        SFSpeechRecognizerAuthorizationStatus::Authorized => MacOsSpeechAuthorization::Authorized,
        _ => MacOsSpeechAuthorization::Denied,
    }
}

fn speech_recognizer(locale: Option<&str>) -> Option<objc2::rc::Retained<SFSpeechRecognizer>> {
    let recognizer = match locale {
        Some(identifier) => {
            let identifier = NSString::from_str(identifier);
            let locale = NSLocale::initWithLocaleIdentifier(NSLocale::alloc(), &identifier);
            unsafe { SFSpeechRecognizer::initWithLocale(SFSpeechRecognizer::alloc(), &locale) }
        }
        None => unsafe { SFSpeechRecognizer::init(SFSpeechRecognizer::alloc()) },
    }?;
    let queue = NSOperationQueue::new();
    queue.setMaxConcurrentOperationCount(1);
    unsafe { recognizer.setQueue(&queue) };
    Some(recognizer)
}

fn configure_request(
    request: &SFSpeechAudioBufferRecognitionRequest,
    hints: &SpeechRecognitionHints,
) {
    unsafe {
        request.setShouldReportPartialResults(true);
        request.setAddsPunctuation(true);
    }
    let terms = hints
        .terms()
        .iter()
        .map(|term| term.trim())
        .filter(|term| !term.is_empty())
        .take(MAX_CONTEXTUAL_STRINGS)
        .map(NSString::from_str)
        .collect::<Vec<_>>();
    if !terms.is_empty() {
        let terms = NSArray::from_retained_slice(&terms);
        unsafe { request.setContextualStrings(&terms) };
    }
}

fn process_events(
    request: &SFSpeechAudioBufferRecognitionRequest,
    event_rx: &mpsc::Receiver<WorkerEvent>,
    on_partial: Arc<dyn Fn(String) + Send + Sync>,
) -> Option<Result<String, SpeechRecognitionError>> {
    let mut finalization_started = None;
    loop {
        let event = match finalization_started {
            Some(started) => {
                let remaining =
                    FINAL_TIMEOUT.saturating_sub(Instant::now().duration_since(started));
                if remaining.is_zero() {
                    return Some(Err(SpeechRecognitionError::Timeout));
                }
                event_rx
                    .recv_timeout(remaining)
                    .map_err(|error| match error {
                        mpsc::RecvTimeoutError::Timeout => SpeechRecognitionError::Timeout,
                        mpsc::RecvTimeoutError::Disconnected => SpeechRecognitionError::Transport(
                            "Apple Speech session disconnected".to_owned(),
                        ),
                    })
            }
            None => event_rx.recv().map_err(|_| {
                SpeechRecognitionError::Transport("Apple Speech session disconnected".to_owned())
            }),
        };
        let event = match event {
            Ok(event) => event,
            Err(error) => return Some(Err(error)),
        };
        match event {
            WorkerEvent::Audio(samples) if finalization_started.is_none() => {
                if let Err(error) = append_samples(request, &samples) {
                    return Some(Err(error));
                }
            }
            WorkerEvent::Audio(_) => {}
            WorkerEvent::Finish if finalization_started.is_none() => {
                unsafe { request.endAudio() };
                finalization_started = Some(Instant::now());
            }
            WorkerEvent::Finish => {}
            WorkerEvent::Cancel => return None,
            WorkerEvent::Recognition(event) => {
                if let Some(error) = event.error {
                    return Some(Err(error));
                }
                if let Some(text) = event.text {
                    if event.is_final {
                        return Some(final_transcript(text));
                    }
                    if !text.trim().is_empty() {
                        on_partial(text);
                    }
                }
            }
        }
    }
}

fn final_transcript(text: String) -> Result<String, SpeechRecognitionError> {
    let text = text.trim().to_owned();
    if text.is_empty() {
        return Err(SpeechRecognitionError::Protocol(
            "Apple Speech returned an empty final transcript".to_owned(),
        ));
    }
    Ok(text)
}

fn append_samples(
    request: &SFSpeechAudioBufferRecognitionRequest,
    samples: &[i16],
) -> Result<(), SpeechRecognitionError> {
    if samples.is_empty() {
        return Ok(());
    }
    let frame_count = u32::try_from(samples.len()).map_err(protocol_error)?;
    let format = unsafe {
        AVAudioFormat::initWithCommonFormat_sampleRate_channels_interleaved(
            AVAudioFormat::alloc(),
            AVAudioCommonFormat::PCMFormatInt16,
            f64::from(SAMPLE_RATE),
            1,
            false,
        )
    }
    .ok_or_else(|| SpeechRecognitionError::Protocol("could not create PCM format".to_owned()))?;
    let buffer = unsafe {
        AVAudioPCMBuffer::initWithPCMFormat_frameCapacity(
            AVAudioPCMBuffer::alloc(),
            &format,
            frame_count,
        )
    }
    .ok_or_else(|| SpeechRecognitionError::Protocol("could not create PCM buffer".to_owned()))?;
    let channels = unsafe { buffer.int16ChannelData() };
    let channel = NonNull::new(channels)
        .map(|channels| unsafe { *channels.as_ptr() })
        .ok_or_else(|| {
            SpeechRecognitionError::Protocol("PCM buffer has no 16-bit channel data".to_owned())
        })?;
    unsafe {
        std::ptr::copy_nonoverlapping(samples.as_ptr(), channel.as_ptr(), samples.len());
        buffer.setFrameLength(frame_count);
        request.appendAudioPCMBuffer(&buffer);
    }
    Ok(())
}

fn result_handler(sender: mpsc::Sender<WorkerEvent>) -> AppleResultHandler {
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
            let _ = sender.send(WorkerEvent::Recognition(event));
        },
    )
}

fn apple_error(error: &NSError) -> SpeechRecognitionError {
    SpeechRecognitionError::Transport(format!(
        "Apple Speech {} ({}): {}",
        error.domain(),
        error.code(),
        error.localizedDescription()
    ))
}

fn protocol_error(error: impl std::fmt::Display) -> SpeechRecognitionError {
    SpeechRecognitionError::Protocol(error.to_string())
}

fn transport_error(error: impl std::fmt::Display) -> SpeechRecognitionError {
    SpeechRecognitionError::Transport(error.to_string())
}

struct MacOsSpeechSession {
    event_tx: mpsc::Sender<WorkerEvent>,
    result_rx: mpsc::Receiver<Result<String, SpeechRecognitionError>>,
    closed: AtomicBool,
}

impl StreamingRecognitionSession for MacOsSpeechSession {
    fn push_audio(&self, samples: Vec<i16>) -> Result<(), SpeechRecognitionError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(SpeechRecognitionError::Transport(
                "Apple Speech session is closed".to_owned(),
            ));
        }
        self.event_tx
            .send(WorkerEvent::Audio(samples))
            .map_err(|_| {
                SpeechRecognitionError::Transport("Apple Speech worker stopped".to_owned())
            })
    }

    fn finish(self: Box<Self>) -> Result<String, SpeechRecognitionError> {
        self.closed.store(true, Ordering::Release);
        let _ = self.event_tx.send(WorkerEvent::Finish);
        self.result_rx
            .recv_timeout(FINAL_TIMEOUT + Duration::from_secs(2))
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => SpeechRecognitionError::Timeout,
                mpsc::RecvTimeoutError::Disconnected => {
                    SpeechRecognitionError::Transport("Apple Speech worker stopped".to_owned())
                }
            })?
    }

    fn cancel(self: Box<Self>) {
        self.closed.store(true, Ordering::Release);
        let _ = self.event_tx.send(WorkerEvent::Cancel);
    }
}

impl Drop for MacOsSpeechSession {
    fn drop(&mut self) {
        if !self.closed.swap(true, Ordering::AcqRel) {
            let _ = self.event_tx.send(WorkerEvent::Cancel);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_session() -> (
        Box<MacOsSpeechSession>,
        mpsc::Receiver<WorkerEvent>,
        mpsc::SyncSender<Result<String, SpeechRecognitionError>>,
    ) {
        let (event_tx, event_rx) = mpsc::channel();
        let (result_tx, result_rx) = mpsc::sync_channel(1);
        (
            Box::new(MacOsSpeechSession {
                event_tx,
                result_rx,
                closed: AtomicBool::new(false),
            }),
            event_rx,
            result_tx,
        )
    }

    #[test]
    fn session_preserves_audio_chunks() {
        let (session, events, _results) = test_session();

        assert_eq!(Ok(()), session.push_audio(vec![1, -2, 3]));
        let received = events.recv_timeout(Duration::from_secs(1));

        assert!(matches!(received, Ok(WorkerEvent::Audio(samples)) if samples == [1, -2, 3]));
        session.cancel();
    }

    #[test]
    fn finish_returns_the_worker_final_transcript() {
        let (session, events, results) = test_session();
        let worker = thread::spawn(move || {
            assert!(matches!(events.recv(), Ok(WorkerEvent::Finish)));
            let _ = results.send(Ok("最终文本".to_owned()));
        });

        assert_eq!(Ok("最终文本".to_owned()), session.finish());
        assert!(worker.join().is_ok());
    }

    #[test]
    fn finish_returns_a_final_transcript_after_the_worker_exits_early() {
        let (session, events, results) = test_session();
        let _ = results.send(Ok("提前完成".to_owned()));
        drop(events);

        assert_eq!(Ok("提前完成".to_owned()), session.finish());
    }

    #[test]
    fn cancel_notifies_the_worker_without_waiting_for_a_result() {
        let (session, events, _results) = test_session();

        session.cancel();

        assert!(matches!(events.recv(), Ok(WorkerEvent::Cancel)));
    }

    #[test]
    fn denied_and_restricted_authorization_map_to_authentication() {
        assert_eq!(
            Err(SpeechRecognitionError::Authentication),
            authorization_result(SFSpeechRecognizerAuthorizationStatus::Denied)
        );
        assert_eq!(
            Err(SpeechRecognitionError::Authentication),
            authorization_result(SFSpeechRecognizerAuthorizationStatus::Restricted)
        );
    }

    #[test]
    fn explicit_locale_rejects_empty_values() {
        assert!(matches!(
            MacOsSpeechRecognizer::for_locale("  "),
            Err(SpeechRecognitionError::Protocol(_))
        ));
    }

    #[test]
    fn empty_final_transcripts_are_protocol_errors() {
        assert!(matches!(
            final_transcript("  ".to_owned()),
            Err(SpeechRecognitionError::Protocol(_))
        ));
    }
}
