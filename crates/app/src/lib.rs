#![cfg_attr(test, allow(clippy::panic))]

mod audio_recording;
mod cancelled_recording;
mod dictation_completion;
mod dictation_session;
mod dictionary_learning;
mod feedback_sound;
mod final_text_processing;
mod local_settings_mutation;
mod refinement_policy;
mod refinement_prompt;
mod refinement_terms;
mod settings;
mod speech_recognition;
mod storage;
mod text_delivery;
mod usage_summary;

pub use audio_recording::{
    AudioInputDevice, AudioRecorder, MicrophoneAuthorization, MicrophonePermissionProvider,
    PcmChunk, PcmRecording, RecordingError, RecordingMetrics, RecordingStarted, TARGET_SAMPLE_RATE,
    convert_interleaved_f32_to_pcm16,
};
pub use cancelled_recording::CancelledRecordingStore;
pub use dictation_completion::{
    CompletedDictation, DictationCompletion, DictationCompletionAdapters, DictationCompletionClock,
    DictationCompletionError, DictationCompletionPolicy, DictationCompletionResult,
    DictationHandoff, DictationHistoryMetadata, DictationHistoryPolicy, DictationHistoryResult,
    DictationHistorySkipReason, DictationHistoryWriter, DictationPolicyError,
    DictationPolicySource, DictationSessionId, FailedDictation, FinalTranscriptRefiner,
    OwnedRecognition, RestoredRecordingTranscriber,
};
pub use dictation_session::{DictationSession, DictationSessionState, DictationToggleAction};
pub use dictionary_learning::{
    CandidateAssessmentSource, CandidateDecision, DictionaryCandidateAssessment,
    DictionaryCandidateEvidence, DictionaryCandidateKind, DictionaryCandidateState,
    DictionaryCorrection, DictionaryLearningOutcome, DictionaryLearningStore,
    NewDictionaryObservation, assess_dictionary_candidate, correction_from_edit,
    parse_dictionary_candidate_review, review_dictionary_candidate,
};
pub use feedback_sound::{FeedbackSound, FeedbackSoundError, FeedbackSoundPlayer};
pub use final_text_processing::{
    FinalTextProcessingError, FinalTextProcessor, FinalTextRequest, LlmProvider, LlmProviderError,
    LlmRefinementRequest, ProcessedText, RefinementEvaluation, RefinementEvaluationMode,
    RefinementFallbackReason, RefinementMode, RefinementSkipReason, RefinementStatus,
    RefinementTerm, refinement_needed,
};
pub use local_settings_mutation::{
    LocalSettingsChange, LocalSettingsMutationError, LocalSettingsMutator,
    LocalSettingsValidationError, MicrophoneSelection,
};
pub use refinement_terms::{
    dictionary_terms_for_refinement, dictionary_terms_for_refinement_from_entries,
    normalize_standard_spellings, standard_spelling_occurs,
};
pub use settings::{
    ActiveProviders, AsrSettings, ChatCompletionsLlmSettings, LlmProviderPreset, LlmSettings,
    OpenAiCompatibleAsrSettings, ProviderCatalog, ProviderConfigStore, ProviderDataConsent,
    ProviderInstance, SaymoreSettings, SettingsStore, SettingsStoreError, VolcengineAsrSettings,
};
pub use speech_recognition::{
    SpeechRecognitionError, SpeechRecognitionHints, StreamingRecognitionSession,
    StreamingSpeechRecognizer,
};
pub use storage::{
    DictionaryEntry, DictionaryOrigin, DictionaryStore, HistoryCursor, HistoryDelivery,
    HistoryPage, HistoryRecord, HistoryRefinement, HistoryRetention, HistoryStore, InstalledModel,
    InstalledModelStore, LocalSettings, LocalSettingsStore, NewDictionaryEntry, NewHistoryRecord,
    OnboardingStatus, OnboardingStep, SecretStore, SecretStoreError, StorageError,
    UiLanguagePreference, dictionary_comparison_key, normalize_language_tag,
};
pub use text_delivery::{
    AccessibilityAuthorization, CorrectionObservingTextDeliverer, DeliveryTargetPrivacy,
    ObservedTextEdit, TextDeliverer, TextDeliveryError, TextDeliveryOutcome, TextEditObserver,
};
pub use usage_summary::{USAGE_TREND_DAYS, UsageSummary, load_usage_summary};
