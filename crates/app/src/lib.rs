#![cfg_attr(test, allow(clippy::panic))]

use template_domain::Greeting;

mod audio_recording;
mod cancelled_recording;
mod feedback_sound;
mod settings;
mod speech_recognition;
mod text_delivery;

pub use audio_recording::{
    AudioRecorder, MicrophoneAuthorization, MicrophonePermissionProvider, PcmChunk, PcmRecording,
    RecordingError, RecordingMetrics, RecordingStarted, TARGET_SAMPLE_RATE,
    convert_interleaved_f32_to_pcm16,
};
pub use cancelled_recording::CancelledRecordingStore;
pub use feedback_sound::{FeedbackSound, FeedbackSoundError, FeedbackSoundPlayer};
pub use settings::{
    AsrSettings, SaymoreSettings, SettingsStore, SettingsStoreError, VolcengineAsrSettings,
};
pub use speech_recognition::{
    SpeechRecognitionError, StreamingRecognitionSession, StreamingSpeechRecognizer,
};
pub use text_delivery::{
    AccessibilityAuthorization, TextDeliverer, TextDeliveryError, TextDeliveryOutcome,
};

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
