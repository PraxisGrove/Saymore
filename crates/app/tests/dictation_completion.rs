use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use template_app::{
    AccessibilityAuthorization, CompletedDictation, DictationCompletion,
    DictationCompletionAdapters, DictationCompletionClock, DictationCompletionPolicy,
    DictationCompletionResult, DictationHandoff, DictationHistoryPolicy, DictationHistoryResult,
    DictationHistorySkipReason, DictationHistoryWriter, DictationPolicyError,
    DictationPolicySource, DictationSessionId, DictionaryEntry, DictionaryOrigin, DictionaryStore,
    FinalTextProcessingError, FinalTextRequest, FinalTranscriptRefiner, HistoryDelivery,
    HistoryRefinement, NewDictionaryEntry, NewHistoryRecord, OwnedRecognition, PcmRecording,
    ProcessedText, RefinementEvaluation, RefinementFallbackReason, RefinementMode,
    RefinementStatus, RefinementTerm, RestoredRecordingTranscriber, SpeechRecognitionError,
    StorageError, StreamingRecognitionSession, TextDeliverer, TextDeliveryError,
    TextDeliveryOutcome,
};

struct Scenario {
    policy: Result<DictationCompletionPolicy, DictationPolicyError>,
    restored: Result<String, SpeechRecognitionError>,
    refinement: Result<RefinementEvaluation, FinalTextProcessingError>,
    dictionary: Result<Vec<DictionaryEntry>, StorageError>,
    delivery: Result<TextDeliveryOutcome, TextDeliveryError>,
    history: Result<(), StorageError>,
    now_ms: i64,
}

impl Default for Scenario {
    fn default() -> Self {
        Self {
            policy: Ok(policy(
                RefinementMode::Disabled,
                DictationHistoryPolicy::Disabled,
            )),
            restored: Ok("restored transcript".to_owned()),
            refinement: Err(FinalTextProcessingError::Cancelled),
            dictionary: Ok(Vec::new()),
            delivery: Ok(TextDeliveryOutcome::AccessibilityVerified),
            history: Ok(()),
            now_ms: 1_750_000_000_000,
        }
    }
}

struct Harness {
    completion: DictationCompletion,
    adapter: Arc<TestAdapter>,
}

impl Harness {
    fn new(scenario: Scenario) -> Self {
        let adapter = Arc::new(TestAdapter {
            scenario,
            policy_calls: AtomicUsize::new(0),
            privacy_checks: AtomicUsize::new(0),
            refinement_requests: Mutex::new(Vec::new()),
            restored_calls: Mutex::new(Vec::new()),
            delivered: Mutex::new(Vec::new()),
            history_records: Mutex::new(Vec::new()),
        });
        let completion = DictationCompletion::new(DictationCompletionAdapters {
            policy: adapter.clone(),
            restored_transcriber: adapter.clone(),
            refiner: adapter.clone(),
            dictionary: adapter.clone(),
            deliverer: adapter.clone(),
            history: adapter.clone(),
            clock: adapter.clone(),
        });
        Self {
            completion,
            adapter,
        }
    }
}

struct TestAdapter {
    scenario: Scenario,
    policy_calls: AtomicUsize,
    privacy_checks: AtomicUsize,
    refinement_requests: Mutex<Vec<FinalTextRequest>>,
    restored_calls: Mutex<Vec<(DictationSessionId, PcmRecording)>>,
    delivered: Mutex<Vec<String>>,
    history_records: Mutex<Vec<NewHistoryRecord>>,
}

impl DictationPolicySource for TestAdapter {
    fn load_policy(&self) -> Result<DictationCompletionPolicy, DictationPolicyError> {
        self.policy_calls.fetch_add(1, Ordering::Relaxed);
        self.scenario.policy.clone()
    }
}

impl RestoredRecordingTranscriber for TestAdapter {
    fn transcribe(
        &self,
        id: DictationSessionId,
        recording: &PcmRecording,
    ) -> Result<String, SpeechRecognitionError> {
        self.restored_calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((id, recording.clone()));
        self.scenario.restored.clone()
    }
}

impl FinalTranscriptRefiner for TestAdapter {
    fn refine(
        &self,
        _id: DictationSessionId,
        request: FinalTextRequest,
    ) -> Result<RefinementEvaluation, FinalTextProcessingError> {
        self.refinement_requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(request);
        self.scenario.refinement.clone()
    }
}

impl DictionaryStore for TestAdapter {
    fn list_dictionary(&self) -> Result<Vec<DictionaryEntry>, StorageError> {
        self.scenario.dictionary.clone()
    }

    fn upsert_dictionary(
        &self,
        _entry: NewDictionaryEntry,
        _now_ms: i64,
    ) -> Result<DictionaryEntry, StorageError> {
        Err(StorageError::Unavailable(
            "unexpected dictionary mutation".to_owned(),
        ))
    }

    fn delete_dictionary(&self, _id: &str) -> Result<(), StorageError> {
        Err(StorageError::Unavailable(
            "unexpected dictionary deletion".to_owned(),
        ))
    }
}

impl TextDeliverer for TestAdapter {
    fn authorization(&self) -> AccessibilityAuthorization {
        AccessibilityAuthorization::Granted
    }

    fn request_authorization(&self) -> AccessibilityAuthorization {
        AccessibilityAuthorization::Granted
    }

    fn target_privacy(&self) -> template_app::DeliveryTargetPrivacy {
        self.privacy_checks.fetch_add(1, Ordering::Relaxed);
        template_app::DeliveryTargetPrivacy::Sensitive
    }

    fn deliver(&self, text: &str) -> Result<TextDeliveryOutcome, TextDeliveryError> {
        self.delivered
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(text.to_owned());
        self.scenario.delivery.clone()
    }
}

impl DictationHistoryWriter for TestAdapter {
    fn insert_history(&self, record: NewHistoryRecord) -> Result<(), StorageError> {
        self.history_records
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(record);
        self.scenario.history.clone()
    }
}

impl DictationCompletionClock for TestAdapter {
    fn now_ms(&self) -> i64 {
        self.scenario.now_ms
    }
}

struct FinishingRecognition {
    finish_calls: Arc<AtomicUsize>,
    transcript: String,
}

impl StreamingRecognitionSession for FinishingRecognition {
    fn push_audio(&self, _samples: Vec<i16>) -> Result<(), SpeechRecognitionError> {
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<String, SpeechRecognitionError> {
        self.finish_calls.fetch_add(1, Ordering::Relaxed);
        Ok(self.transcript)
    }

    fn cancel(self: Box<Self>) {}
}

#[test]
fn captured_dictation_finishes_recognition_and_delivers_once() {
    let id = DictationSessionId::generate();
    let finish_calls = Arc::new(AtomicUsize::new(0));
    let harness = Harness::new(Scenario::default());

    let result = harness.completion.complete(DictationHandoff::Captured {
        id,
        recording: recording(),
        recognition: OwnedRecognition::new(Box::new(FinishingRecognition {
            finish_calls: Arc::clone(&finish_calls),
            transcript: "  hello   world  ".to_owned(),
        })),
    });

    assert_eq!(
        completed(
            id,
            "hello world",
            RefinementStatus::Disabled,
            Ok(TextDeliveryOutcome::AccessibilityVerified),
            DictationHistoryResult::Skipped(DictationHistorySkipReason::Disabled),
        ),
        result
    );
    assert_eq!(1, finish_calls.load(Ordering::Relaxed));
    assert_eq!(0, harness.adapter.privacy_checks.load(Ordering::Relaxed));
    assert_eq!(vec!["hello world"], strings(&harness.adapter.delivered));
}

#[test]
fn restored_dictation_uses_the_retained_recording_and_identity() {
    let id = DictationSessionId::generate();
    let recording = recording();
    let harness = Harness::new(Scenario::default());

    let result = harness.completion.complete(DictationHandoff::Restored {
        id,
        recording: recording.clone(),
    });

    assert_eq!(
        completed(
            id,
            "restored transcript",
            RefinementStatus::Disabled,
            Ok(TextDeliveryOutcome::AccessibilityVerified),
            DictationHistoryResult::Skipped(DictationHistorySkipReason::Disabled),
        ),
        result
    );
    assert_eq!(
        vec![(id, recording)],
        values(&harness.adapter.restored_calls)
    );
}

#[test]
fn successful_refinement_is_normalized_before_delivery() {
    let id = DictationSessionId::generate();
    let refined = "我正在使用 openai 完成这个更清晰的口述内容";
    let harness = Harness::new(Scenario {
        policy: Ok(policy(
            RefinementMode::Enabled,
            DictationHistoryPolicy::Disabled,
        )),
        refinement: Ok(RefinementEvaluation {
            processed: ProcessedText {
                text: refined.to_owned(),
                refinement: RefinementStatus::Completed,
            },
            provider_output: Some(refined.to_owned()),
        }),
        dictionary: Ok(vec![dictionary_entry("OpenAI")]),
        delivery: Ok(TextDeliveryOutcome::ClipboardVerified),
        ..Scenario::default()
    });
    let transcript = "我正在使用 openai 完成这段比较长的原始口述内容";

    let result = harness
        .completion
        .complete(captured_handoff(id, transcript));

    assert_eq!(
        completed(
            id,
            "我正在使用 OpenAI 完成这个更清晰的口述内容",
            RefinementStatus::Completed,
            Ok(TextDeliveryOutcome::ClipboardVerified),
            DictationHistoryResult::Skipped(DictationHistorySkipReason::Disabled),
        ),
        result
    );
    assert_eq!(
        vec![FinalTextRequest {
            transcript: transcript.to_owned(),
            refinement: RefinementMode::Enabled,
            language: Some("zh-Hans".to_owned()),
            relevant_terms: vec![RefinementTerm {
                canonical: "OpenAI".to_owned(),
            }],
        }],
        values(&harness.adapter.refinement_requests)
    );
}

#[test]
fn refinement_failure_falls_back_without_retrying_delivery() {
    let id = DictationSessionId::generate();
    let text = "this final transcript remains available after provider failure";
    let harness = Harness::new(Scenario {
        policy: Ok(policy(
            RefinementMode::Enabled,
            DictationHistoryPolicy::Disabled,
        )),
        delivery: Err(TextDeliveryError::NoFocusedControl),
        ..Scenario::default()
    });

    let result = harness.completion.complete(captured_handoff(id, text));

    assert_eq!(
        completed(
            id,
            text,
            RefinementStatus::FellBack(RefinementFallbackReason::Protocol),
            Err(TextDeliveryError::NoFocusedControl),
            DictationHistoryResult::Skipped(DictationHistorySkipReason::Disabled),
        ),
        result
    );
    assert_eq!(1, values(&harness.adapter.refinement_requests).len());
    assert_eq!(vec![text], strings(&harness.adapter.delivered));
}

#[test]
fn restricted_delivery_never_writes_history() {
    for delivery in [
        Ok(TextDeliveryOutcome::SecureClipboardAttempted),
        Err(TextDeliveryError::SecureDeliveryFailed(
            "restricted paste failed".to_owned(),
        )),
    ] {
        let id = DictationSessionId::generate();
        let harness = Harness::new(Scenario {
            policy: Ok(policy(
                RefinementMode::Disabled,
                DictationHistoryPolicy::Enabled(Default::default()),
            )),
            delivery: delivery.clone(),
            ..Scenario::default()
        });

        let result = harness
            .completion
            .complete(captured_handoff(id, "private final transcript"));

        assert_eq!(
            completed(
                id,
                "private final transcript",
                RefinementStatus::Disabled,
                delivery,
                DictationHistoryResult::Skipped(DictationHistorySkipReason::SensitiveTarget),
            ),
            result
        );
        assert!(values(&harness.adapter.history_records).is_empty());
        assert_eq!(0, harness.adapter.privacy_checks.load(Ordering::Relaxed));
    }
}

#[test]
fn unavailable_policy_uses_privacy_safe_fallback_and_still_delivers() {
    let id = DictationSessionId::generate();
    let text = "final text survives policy failure";
    let harness = Harness::new(Scenario {
        policy: Err(DictationPolicyError::Unavailable(
            "settings unavailable".to_owned(),
        )),
        ..Scenario::default()
    });

    let result = harness.completion.complete(captured_handoff(id, text));

    assert_eq!(
        completed(
            id,
            text,
            RefinementStatus::Disabled,
            Ok(TextDeliveryOutcome::AccessibilityVerified),
            DictationHistoryResult::Skipped(DictationHistorySkipReason::PolicyUnavailable),
        ),
        result
    );
    assert_eq!(1, harness.adapter.policy_calls.load(Ordering::Relaxed));
    assert!(values(&harness.adapter.refinement_requests).is_empty());
    assert!(values(&harness.adapter.history_records).is_empty());
    assert_eq!(vec![text], strings(&harness.adapter.delivered));
}

#[test]
fn history_failure_does_not_change_the_delivery_terminal_state() {
    let id = DictationSessionId::generate();
    let text = "deliver before history";
    let history_error = StorageError::Unavailable("history worker stopped".to_owned());
    let harness = Harness::new(Scenario {
        policy: Ok(policy(
            RefinementMode::Disabled,
            DictationHistoryPolicy::Enabled(Default::default()),
        )),
        delivery: Ok(TextDeliveryOutcome::ClipboardVerified),
        history: Err(history_error.clone()),
        ..Scenario::default()
    });

    let result = harness.completion.complete(captured_handoff(id, text));

    assert_eq!(
        completed(
            id,
            text,
            RefinementStatus::Disabled,
            Ok(TextDeliveryOutcome::ClipboardVerified),
            DictationHistoryResult::Failed {
                delivery: HistoryDelivery::Delivered,
                error: history_error,
            },
        ),
        result
    );
    assert_eq!(vec![text], strings(&harness.adapter.delivered));
    assert_eq!(
        vec![NewHistoryRecord {
            id: id.to_string(),
            created_at_ms: 1_750_000_000_000,
            final_text: text.to_owned(),
            raw_asr_text: experimental_text(text),
            llm_refined_text: None,
            audio_duration_ms: 30,
            language: None,
            delivery: HistoryDelivery::Delivered,
            refinement: HistoryRefinement::NotUsed,
            asr_provider_id: None,
            llm_provider_id: None,
            asr_model: None,
            llm_model: None,
        }],
        values(&harness.adapter.history_records)
    );
}

fn policy(
    refinement: RefinementMode,
    history: DictationHistoryPolicy,
) -> DictationCompletionPolicy {
    DictationCompletionPolicy {
        refinement,
        history,
    }
}

fn completed(
    id: DictationSessionId,
    text: &str,
    refinement: RefinementStatus,
    delivery: Result<TextDeliveryOutcome, TextDeliveryError>,
    history: DictationHistoryResult,
) -> DictationCompletionResult {
    DictationCompletionResult::Completed(CompletedDictation {
        id,
        processed: ProcessedText {
            text: text.to_owned(),
            refinement,
        },
        delivery,
        history,
    })
}

fn recording() -> PcmRecording {
    PcmRecording {
        samples: vec![1, 2, 3],
        sample_rate: 16_000,
        channels: 1,
        duration_ms: 30,
    }
}

fn captured_handoff(id: DictationSessionId, transcript: &str) -> DictationHandoff {
    DictationHandoff::Captured {
        id,
        recording: recording(),
        recognition: OwnedRecognition::new(Box::new(FinishingRecognition {
            finish_calls: Arc::new(AtomicUsize::new(0)),
            transcript: transcript.to_owned(),
        })),
    }
}

fn dictionary_entry(canonical: &str) -> DictionaryEntry {
    DictionaryEntry {
        id: canonical.to_owned(),
        canonical: canonical.to_owned(),
        language: "en".to_owned(),
        origin: DictionaryOrigin::Manual,
        created_at_ms: 1,
        updated_at_ms: 1,
    }
}

fn values<T: Clone>(values: &Mutex<Vec<T>>) -> Vec<T> {
    values
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

fn strings(recorded: &Mutex<Vec<String>>) -> Vec<String> {
    values(recorded)
}

fn experimental_text(text: &str) -> Option<String> {
    cfg!(any(debug_assertions, feature = "history-experiments")).then(|| text.to_owned())
}
