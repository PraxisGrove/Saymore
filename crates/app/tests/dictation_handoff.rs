use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};

use template_app::{
    CancelledRecordingStore, DictationHandoff, DictationSession, DictationSessionId,
    DictationToggleAction, OwnedRecognition, PcmRecording, SpeechRecognitionError,
    StreamingRecognitionSession,
};

struct FakeRecognitionSession {
    finish_calls: Arc<AtomicUsize>,
    cancel_calls: Arc<AtomicUsize>,
}

impl StreamingRecognitionSession for FakeRecognitionSession {
    fn push_audio(&self, _samples: Vec<i16>) -> Result<(), SpeechRecognitionError> {
        Err(SpeechRecognitionError::Transport(
            "stream closed".to_owned(),
        ))
    }

    fn finish(self: Box<Self>) -> Result<String, SpeechRecognitionError> {
        self.finish_calls.fetch_add(1, Ordering::Relaxed);
        Err(SpeechRecognitionError::Authentication)
    }

    fn cancel(self: Box<Self>) {
        self.cancel_calls.fetch_add(1, Ordering::Relaxed);
    }
}

#[test]
fn owned_recognition_prioritizes_provider_finish_error_over_stream_error() {
    let finish_calls = Arc::new(AtomicUsize::new(0));
    let cancel_calls = Arc::new(AtomicUsize::new(0));
    let mut recognition = OwnedRecognition::new(Box::new(FakeRecognitionSession {
        finish_calls: Arc::clone(&finish_calls),
        cancel_calls,
    }));

    assert_eq!(
        Err(SpeechRecognitionError::Transport(
            "stream closed".to_owned()
        )),
        recognition.push_audio(vec![1, 2, 3])
    );
    assert_eq!(
        Err(SpeechRecognitionError::Authentication),
        recognition.finish()
    );
    assert_eq!(1, finish_calls.load(Ordering::Relaxed));
}

#[test]
fn owned_recognition_cancels_the_provider_once() {
    let cancel_calls = Arc::new(AtomicUsize::new(0));
    let recognition = OwnedRecognition::new(Box::new(FakeRecognitionSession {
        finish_calls: Arc::new(AtomicUsize::new(0)),
        cancel_calls: Arc::clone(&cancel_calls),
    }));

    recognition.cancel();

    assert_eq!(1, cancel_calls.load(Ordering::Relaxed));
}

#[test]
fn cancelled_recording_restores_the_same_session_identity() {
    let now = Instant::now();
    let id = DictationSessionId::generate();
    let recording = PcmRecording {
        samples: vec![1, 2, 3],
        sample_rate: 16_000,
        channels: 1,
        duration_ms: 30,
    };
    let mut store = CancelledRecordingStore::new(Duration::from_secs(10));

    store.retain(id, recording.clone(), now);

    assert!(matches!(
        store.take(now + Duration::from_secs(9)),
        Some(DictationHandoff::Restored {
            id: restored_id,
            recording: restored_recording,
        }) if restored_id == id && restored_recording == recording
    ));
}

#[test]
fn dictation_identity_is_stable_until_the_session_completes() {
    let session = DictationSession::default();

    assert_eq!(DictationToggleAction::Start, session.request_toggle(false));
    let id = session.current_id();
    assert!(id.is_some());
    assert!(session.recording_started());
    assert_eq!(id, session.request_finish());
    session.complete();

    assert_eq!(None, session.current_id());
}
