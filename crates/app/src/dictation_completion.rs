use std::{fmt, sync::Arc};

use thiserror::Error;
use uuid::Uuid;

use crate::{
    DictionaryStore, FinalTextProcessingError, FinalTextRequest, HistoryDelivery,
    HistoryRefinement, NewHistoryRecord, PcmRecording, ProcessedText, RecordingError,
    RefinementEvaluation, RefinementFallbackReason, RefinementMode, RefinementStatus,
    SpeechRecognitionError, StorageError, StreamingRecognitionSession, TextDeliverer,
    TextDeliveryError, TextDeliveryOutcome, dictionary_terms_for_refinement,
    normalize_standard_spellings,
};

/// Stable identity shared by every fact produced for one dictation session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DictationSessionId(Uuid);

impl DictationSessionId {
    pub fn generate() -> Self {
        Self(Uuid::new_v4())
    }
}

impl fmt::Display for DictationSessionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Owns one active recognition session after a caller hands it to completion.
///
/// Callers push ordered audio while recording, then transfer this value so the
/// receiver must consume it through exactly one `finish` or `cancel` call.
pub struct OwnedRecognition {
    session: Box<dyn StreamingRecognitionSession>,
    stream_error: Option<SpeechRecognitionError>,
}

impl OwnedRecognition {
    pub fn new(session: Box<dyn StreamingRecognitionSession>) -> Self {
        Self {
            session,
            stream_error: None,
        }
    }

    pub fn push_audio(&mut self, samples: Vec<i16>) -> Result<(), SpeechRecognitionError> {
        let result = self.session.push_audio(samples);
        if let Err(error) = &result
            && self.stream_error.is_none()
        {
            self.stream_error = Some(error.clone());
        }
        result
    }

    pub fn finish(self) -> Result<String, SpeechRecognitionError> {
        match (self.stream_error, self.session.finish()) {
            (_, Err(provider_error)) => Err(provider_error),
            (Some(stream_error), Ok(_)) => Err(stream_error),
            (None, Ok(transcript)) => Ok(transcript),
        }
    }

    pub fn cancel(self) {
        self.session.cancel();
    }
}

/// Transfers a completed capture attempt into dictation completion.
pub enum DictationHandoff {
    Captured {
        id: DictationSessionId,
        recording: PcmRecording,
        recognition: OwnedRecognition,
    },
    Restored {
        id: DictationSessionId,
        recording: PcmRecording,
    },
    CaptureFailed {
        id: DictationSessionId,
        error: RecordingError,
        recognition: Option<OwnedRecognition>,
    },
}

impl DictationHandoff {
    pub fn id(&self) -> DictationSessionId {
        match self {
            Self::Captured { id, .. }
            | Self::Restored { id, .. }
            | Self::CaptureFailed { id, .. } => *id,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DictationHistoryMetadata {
    pub asr_provider_id: Option<String>,
    pub llm_provider_id: Option<String>,
    pub asr_model: Option<String>,
    pub llm_model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DictationHistoryPolicy {
    Disabled,
    Enabled(DictationHistoryMetadata),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictationCompletionPolicy {
    pub refinement: RefinementMode,
    pub history: DictationHistoryPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DictationPolicyError {
    #[error("dictation completion policy is unavailable: {0}")]
    Unavailable(String),
}

/// Loads one immutable policy snapshot for a dictation after recognition succeeds.
///
/// Implementations may combine local settings and provider metadata, but must not
/// resolve or inspect the focused delivery target.
pub trait DictationPolicySource: Send + Sync {
    fn load_policy(&self) -> Result<DictationCompletionPolicy, DictationPolicyError>;
}

/// Recognizes retained audio when a cancelled dictation is restored.
///
/// Implementations must consume the complete recording in order and return one
/// final transcript without retaining the recording after this call returns.
pub trait RestoredRecordingTranscriber: Send + Sync {
    fn transcribe(
        &self,
        id: DictationSessionId,
        recording: &PcmRecording,
    ) -> Result<String, SpeechRecognitionError>;
}

/// Runs the optional provider-backed transformation of one final transcript.
///
/// Implementations perform a synchronous one-shot call from the completion worker.
/// They must not normalize dictionary spellings, deliver text, or persist history.
pub trait FinalTranscriptRefiner: Send + Sync {
    fn refine(
        &self,
        id: DictationSessionId,
        request: FinalTextRequest,
    ) -> Result<RefinementEvaluation, FinalTextProcessingError>;
}

/// Persists one final encrypted history record after its delivery outcome is known.
///
/// Implementations must preserve the record identity so repeated writes for the
/// same dictation remain idempotent.
pub trait DictationHistoryWriter: Send + Sync {
    fn insert_history(&self, record: NewHistoryRecord) -> Result<(), StorageError>;
}

/// Supplies UTC Unix timestamps for completed dictation history records.
///
/// Implementations should read the current clock once per call and saturate values
/// that cannot be represented as signed milliseconds.
pub trait DictationCompletionClock: Send + Sync {
    fn now_ms(&self) -> i64;
}

pub struct DictationCompletionAdapters {
    pub policy: Arc<dyn DictationPolicySource>,
    pub restored_transcriber: Arc<dyn RestoredRecordingTranscriber>,
    pub refiner: Arc<dyn FinalTranscriptRefiner>,
    pub dictionary: Arc<dyn DictionaryStore>,
    pub deliverer: Arc<dyn TextDeliverer>,
    pub history: Arc<dyn DictationHistoryWriter>,
    pub clock: Arc<dyn DictationCompletionClock>,
}

pub struct DictationCompletion {
    adapters: DictationCompletionAdapters,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DictationHistorySkipReason {
    Disabled,
    PolicyUnavailable,
    SensitiveTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DictationHistoryResult {
    Skipped(DictationHistorySkipReason),
    Saved(HistoryDelivery),
    Failed {
        delivery: HistoryDelivery,
        error: StorageError,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Terminal output consumed by desktop presentation after delivery and history handling.
pub struct CompletedDictation {
    pub id: DictationSessionId,
    /// Duration retained for desktop completion presentation after raw audio is dropped.
    pub audio_duration_ms: u64,
    pub processed: ProcessedText,
    pub delivery: Result<TextDeliveryOutcome, TextDeliveryError>,
    pub history: DictationHistoryResult,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DictationCompletionError {
    Recording(RecordingError),
    Recognition(SpeechRecognitionError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedDictation {
    pub id: DictationSessionId,
    pub error: DictationCompletionError,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DictationCompletionResult {
    Completed(CompletedDictation),
    Failed(FailedDictation),
}

impl DictationCompletion {
    pub fn new(adapters: DictationCompletionAdapters) -> Self {
        Self { adapters }
    }

    pub fn complete(&self, handoff: DictationHandoff) -> DictationCompletionResult {
        let id = handoff.id();
        let finalized =
            match finalize_transcript(handoff, self.adapters.restored_transcriber.as_ref()) {
                Ok(finalized) => finalized,
                Err(error) => {
                    return DictationCompletionResult::Failed(FailedDictation { id, error });
                }
            };
        let transcript = finalized
            .transcript
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if transcript.is_empty() {
            return DictationCompletionResult::Failed(FailedDictation {
                id,
                error: DictationCompletionError::Recognition(SpeechRecognitionError::Protocol(
                    "empty transcript".to_owned(),
                )),
            });
        }

        let policy = self.adapters.policy.load_policy();
        let refinement = match &policy {
            Ok(policy) => policy.refinement.clone(),
            Err(DictationPolicyError::Unavailable(_)) => RefinementMode::Disabled,
        };
        let terms =
            dictionary_terms_for_refinement(self.adapters.dictionary.as_ref()).unwrap_or_default();
        let (mut processed, llm_refined_text) =
            self.process_transcript(id, transcript.clone(), refinement, terms.clone());
        processed.text = normalize_standard_spellings(&processed.text, &terms);

        let delivery = self.adapters.deliverer.deliver(&processed.text);
        let audio_duration_ms = finalized.recording.duration_ms;
        let history = self.persist_history(
            id,
            &finalized.recording,
            &transcript,
            llm_refined_text.as_deref(),
            &processed,
            &delivery,
            policy,
        );

        DictationCompletionResult::Completed(CompletedDictation {
            id,
            audio_duration_ms,
            processed,
            delivery,
            history,
        })
    }

    fn process_transcript(
        &self,
        id: DictationSessionId,
        transcript: String,
        refinement: RefinementMode,
        relevant_terms: Vec<crate::RefinementTerm>,
    ) -> (ProcessedText, Option<String>) {
        if refinement == RefinementMode::Disabled {
            return (
                ProcessedText {
                    text: transcript,
                    refinement: RefinementStatus::Disabled,
                },
                None,
            );
        }
        let fallback_text = transcript.clone();
        let mut request = FinalTextRequest::new(transcript, refinement);
        request.language = Some(inferred_transcript_language(&request.transcript).to_owned());
        request.relevant_terms = relevant_terms;
        match self.adapters.refiner.refine(id, request) {
            Ok(evaluation) => {
                let llm_refined_text =
                    matches!(evaluation.processed.refinement, RefinementStatus::Completed)
                        .then(|| evaluation.processed.text.clone());
                (evaluation.processed, llm_refined_text)
            }
            Err(FinalTextProcessingError::Cancelled) => (
                ProcessedText {
                    text: fallback_text,
                    refinement: RefinementStatus::FellBack(RefinementFallbackReason::Protocol),
                },
                None,
            ),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn persist_history(
        &self,
        id: DictationSessionId,
        recording: &PcmRecording,
        raw_asr_text: &str,
        llm_refined_text: Option<&str>,
        processed: &ProcessedText,
        delivery: &Result<TextDeliveryOutcome, TextDeliveryError>,
        policy: Result<DictationCompletionPolicy, DictationPolicyError>,
    ) -> DictationHistoryResult {
        let policy = match policy {
            Ok(policy) => policy,
            Err(DictationPolicyError::Unavailable(_)) => {
                return DictationHistoryResult::Skipped(
                    DictationHistorySkipReason::PolicyUnavailable,
                );
            }
        };
        let metadata = match policy.history {
            DictationHistoryPolicy::Disabled => {
                return DictationHistoryResult::Skipped(DictationHistorySkipReason::Disabled);
            }
            DictationHistoryPolicy::Enabled(metadata) => metadata,
        };
        let Some(history_delivery) = history_delivery(delivery) else {
            return DictationHistoryResult::Skipped(DictationHistorySkipReason::SensitiveTarget);
        };
        let record = NewHistoryRecord {
            id: id.to_string(),
            created_at_ms: self.adapters.clock.now_ms(),
            final_text: processed.text.clone(),
            raw_asr_text: experimental_asr_text(raw_asr_text),
            llm_refined_text: experimental_llm_refined_text(llm_refined_text),
            audio_duration_ms: recording.duration_ms,
            language: None,
            delivery: history_delivery,
            refinement: history_refinement(&processed.refinement),
            asr_provider_id: metadata.asr_provider_id,
            llm_provider_id: metadata.llm_provider_id,
            asr_model: metadata.asr_model,
            llm_model: metadata.llm_model,
        };
        match self.adapters.history.insert_history(record) {
            Ok(()) => DictationHistoryResult::Saved(history_delivery),
            Err(error) => DictationHistoryResult::Failed {
                delivery: history_delivery,
                error,
            },
        }
    }
}

struct FinalizedTranscript {
    recording: PcmRecording,
    transcript: String,
}

fn finalize_transcript(
    handoff: DictationHandoff,
    restored_transcriber: &dyn RestoredRecordingTranscriber,
) -> Result<FinalizedTranscript, DictationCompletionError> {
    match handoff {
        DictationHandoff::Captured {
            recording,
            recognition,
            ..
        } => recognition
            .finish()
            .map(|transcript| FinalizedTranscript {
                recording,
                transcript,
            })
            .map_err(DictationCompletionError::Recognition),
        DictationHandoff::Restored { id, recording } => restored_transcriber
            .transcribe(id, &recording)
            .map(|transcript| FinalizedTranscript {
                recording,
                transcript,
            })
            .map_err(DictationCompletionError::Recognition),
        DictationHandoff::CaptureFailed {
            error, recognition, ..
        } => {
            if let Some(recognition) = recognition {
                recognition.cancel();
            }
            Err(DictationCompletionError::Recording(error))
        }
    }
}

fn inferred_transcript_language(text: &str) -> &'static str {
    if text.chars().any(
        |character| matches!(character as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF),
    ) {
        "zh-Hans"
    } else {
        "en"
    }
}

fn history_delivery(
    delivery: &Result<TextDeliveryOutcome, TextDeliveryError>,
) -> Option<HistoryDelivery> {
    match delivery {
        Ok(TextDeliveryOutcome::SecureClipboardAttempted)
        | Err(TextDeliveryError::SecureDeliveryFailed(_)) => None,
        Ok(TextDeliveryOutcome::AccessibilityVerified | TextDeliveryOutcome::ClipboardVerified) => {
            Some(HistoryDelivery::Delivered)
        }
        Ok(TextDeliveryOutcome::ClipboardAttempted)
        | Err(
            TextDeliveryError::PermissionDenied
            | TextDeliveryError::NoFocusedControl
            | TextDeliveryError::UnsupportedControl
            | TextDeliveryError::AccessibilityUnverified
            | TextDeliveryError::System(_),
        ) => Some(HistoryDelivery::NotDelivered),
    }
}

fn history_refinement(status: &RefinementStatus) -> HistoryRefinement {
    match status {
        RefinementStatus::Disabled | RefinementStatus::Skipped(_) => HistoryRefinement::NotUsed,
        RefinementStatus::Completed => HistoryRefinement::Completed,
        RefinementStatus::FellBack(RefinementFallbackReason::Timeout) => {
            HistoryRefinement::TimedOut
        }
        RefinementStatus::FellBack(RefinementFallbackReason::OutputRejected) => {
            HistoryRefinement::OutputRejected
        }
        RefinementStatus::FellBack(
            RefinementFallbackReason::NotConfigured
            | RefinementFallbackReason::Authentication
            | RefinementFallbackReason::InvalidConfiguration
            | RefinementFallbackReason::ModelUnavailable
            | RefinementFallbackReason::Quota
            | RefinementFallbackReason::Transport
            | RefinementFallbackReason::Protocol
            | RefinementFallbackReason::TemporarilyUnavailable,
        ) => HistoryRefinement::ProviderUnavailable,
    }
}

fn experimental_asr_text(raw_asr_text: &str) -> Option<String> {
    #[cfg(any(debug_assertions, feature = "history-experiments"))]
    {
        Some(raw_asr_text.to_owned())
    }
    #[cfg(not(any(debug_assertions, feature = "history-experiments")))]
    {
        let _ = raw_asr_text;
        None
    }
}

fn experimental_llm_refined_text(llm_refined_text: Option<&str>) -> Option<String> {
    #[cfg(any(debug_assertions, feature = "history-experiments"))]
    {
        llm_refined_text.map(str::to_owned)
    }
    #[cfg(not(any(debug_assertions, feature = "history-experiments")))]
    {
        let _ = llm_refined_text;
        None
    }
}
