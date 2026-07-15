#![cfg_attr(test, allow(clippy::panic))]

use template_domain::Greeting;

mod audio_recording;
mod cancelled_recording;
mod dictionary_learning;
mod feedback_sound;
mod final_text_processing;
mod refinement_policy;
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
pub use dictionary_learning::{
    DictionaryCorrection, DictionaryLearningOutcome, DictionaryLearningStore,
    NewDictionaryObservation, correction_from_edit,
};
pub use feedback_sound::{FeedbackSound, FeedbackSoundError, FeedbackSoundPlayer};
pub use final_text_processing::{
    FinalTextProcessingError, FinalTextProcessor, FinalTextRequest, LlmProvider, LlmProviderError,
    LlmRefinementRequest, ProcessedText, RefinementEvaluation, RefinementEvaluationMode,
    RefinementFallbackReason, RefinementMode, RefinementSkipReason, RefinementStatus,
    RefinementTerm, refinement_needed,
};
pub use refinement_terms::{
    normalize_standard_spellings, relevant_dictionary_terms,
    relevant_dictionary_terms_from_entries, standard_spelling_occurs,
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
    SecretStore, SecretStoreError, StorageError, UiLanguagePreference, dictionary_comparison_key,
    normalize_language_tag,
};
pub use text_delivery::{
    AccessibilityAuthorization, CorrectionObservingTextDeliverer, DeliveryTargetPrivacy,
    ObservedTextEdit, TextDeliverer, TextDeliveryError, TextDeliveryOutcome, TextEditObserver,
};
pub use usage_summary::{USAGE_TREND_DAYS, UsageSummary, load_usage_summary};

pub trait RecipientProvider {
    fn recipient(&self) -> &str;
}

pub fn build_greeting(provider: &impl RecipientProvider) -> String {
    Greeting::new(provider.recipient()).message()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StaticRecipient;

    impl RecipientProvider for StaticRecipient {
        fn recipient(&self) -> &str {
            "workspace"
        }
    }

    #[test]
    fn builds_greeting_from_provider() {
        let message = build_greeting(&StaticRecipient);

        assert_eq!("Hello, workspace!", message);
    }
}
