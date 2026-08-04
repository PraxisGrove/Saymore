#![cfg_attr(test, allow(clippy::panic))]

mod appearance;
mod audio_recording;
mod cancelled_recording;
mod dictation_completion;
mod dictation_session;
mod dictionary_assist;
mod dictionary_learning;
mod dictionary_revision;
mod feedback_sound;
mod final_text_processing;
mod history_revision;
mod local_settings_mutation;
mod local_storage_usage;
mod provider_configuration;
mod refinement_policy;
mod refinement_prompt;
mod refinement_terms;
mod settings;
mod speech_recognition;
mod storage;
mod system_audio;
mod text_delivery;
mod usage_summary;

pub use appearance::{ColorSchemePreference, ThemeId};
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
pub use dictionary_assist::{
    VOCABULARY_SUGGESTION_INSTRUCTIONS, VocabularySuggestionRunError,
    VocabularySuggestionRunOutcome, VocabularySuggestionSample, VocabularySuggestionSettingsStore,
    identify_vocabulary_suggestions, parse_vocabulary_suggestion_response,
    recent_vocabulary_samples, run_vocabulary_suggestions_if_due, vocabulary_suggestion_batches,
    vocabulary_suggestion_consent_fingerprint, vocabulary_suggestions_due,
};
pub use dictionary_learning::{
    CandidateAssessmentSource, CandidateDecision, DictionaryCandidateAssessment,
    DictionaryCandidateEvidence, DictionaryCandidateKind, DictionaryCandidateReview,
    DictionaryCandidateState, DictionaryCorrection, DictionaryLearningOutcome,
    DictionaryLearningStore, NewDictionaryObservation, assess_dictionary_candidate,
    correction_from_edit, parse_dictionary_candidate_review, review_dictionary_candidate,
    review_dictionary_candidate_locally,
};
pub use dictionary_revision::{
    DICTIONARY_REVISION_INSTRUCTIONS, local_dictionary_revision_candidates,
    parse_dictionary_revision_response, review_final_text_revision,
};
pub use feedback_sound::{FeedbackSound, FeedbackSoundError, FeedbackSoundPlayer};
pub use final_text_processing::{
    FinalTextProcessingError, FinalTextProcessor, FinalTextRequest, LlmProvider, LlmProviderError,
    LlmRefinementRequest, ProcessedText, RefinementEvaluation, RefinementEvaluationMode,
    RefinementFallbackReason, RefinementMode, RefinementOutputRejectionReason,
    RefinementSkipReason, RefinementStatus, RefinementTerm, refinement_needed,
};
pub use history_revision::{
    FinalTextRevision, FinalTextRevisionState, TextRevisionDiff, TextRevisionEndReason,
    TextRevisionEvent, TextRevisionObserver, has_text_revision_continuity, text_revision_diffs,
};
pub use local_settings_mutation::{
    LocalSettingsChange, LocalSettingsMutationError, LocalSettingsMutator,
    LocalSettingsValidationError, MicrophoneSelection,
};
pub use local_storage_usage::{LocalModelStorageUsage, LocalStorageUsage};
pub use provider_configuration::{
    AsrConfigurationError, AsrProviderConfiguration, LlmConfigurationError,
    LlmProviderConfiguration, ProviderConfigurationStore, ProviderConfigurator,
    ProviderConnectionTester, llm_consent_required, provider_is_local,
};
pub use refinement_terms::{
    dictionary_terms_for_refinement, dictionary_terms_for_refinement_from_entries,
    normalize_standard_spellings, standard_spelling_occurs,
};
pub use settings::{
    ActiveProviders, AsrSettings, ChatCompletionsLlmSettings, ChatCompletionsProfile,
    LlmModelCatalog, LlmProviderPreset, LlmProviderProfile, LlmSettings,
    OpenAiCompatibleAsrSettings, PARAFORMER_PROVIDER_ID, PARAFORMER_PROVIDER_TYPE,
    ParaformerPunctuationMode, ProviderCatalog, ProviderConfigStore, ProviderDataConsent,
    ProviderInstance, QWEN3_ASR_PROVIDER_ID, QWEN3_ASR_PROVIDER_TYPE, SENSE_VOICE_PROVIDER_ID,
    SENSE_VOICE_PROVIDER_TYPE, SaymoreSettings, SettingsStore, SettingsStoreError,
    VolcengineAsrSettings, WHISPER_PROVIDER_ID, WHISPER_PROVIDER_TYPE,
};
pub use speech_recognition::{
    SpeechRecognitionError, SpeechRecognitionHints, StreamingRecognitionSession,
    StreamingSpeechRecognizer,
};
pub use storage::{
    DailyUsage, DiagnosticEventStore, DictionaryEntry, DictionaryOrigin, DictionaryStore,
    HistoryCursor, HistoryDelivery, HistoryPage, HistoryRecord, HistoryRefinement,
    HistoryRetention, HistoryStore, InstalledModel, InstalledModelStore, LocalSettings,
    LocalSettingsStore, NewDictionaryEntry, NewHistoryRecord, OnboardingStatus, OnboardingStep,
    SecretStore, SecretStoreError, StorageError, UiLanguagePreference, UsageSnapshot, UsageStore,
    dictionary_comparison_key, normalize_language_tag,
};
pub use system_audio::{OutputAudioMuteSession, OutputAudioMuter, SystemAudioMuteError};
pub use text_delivery::{
    AccessibilityAuthorization, CorrectionObservingTextDeliverer, DeliveryTargetPrivacy,
    TextDeliverer, TextDeliveryError, TextDeliveryOutcome,
};
pub use usage_summary::{USAGE_TREND_DAYS, UsageSummary, load_usage_summary};
