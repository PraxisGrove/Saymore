#![cfg_attr(test, allow(clippy::panic))]

use template_app::RecipientProvider;

mod chat_completions_llm;
mod sqlite_storage;

mod app_instance_guard;
mod app_paths;
mod dictionary_files;

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod platform_secret_store;

#[cfg(target_os = "macos")]
mod macos_audio_recorder;

#[cfg(target_os = "macos")]
mod macos_feedback_sound;

#[cfg(target_os = "macos")]
mod macos_main_window;

#[cfg(target_os = "macos")]
mod macos_overlay_window;

#[cfg(target_os = "macos")]
mod macos_settings_store;

#[cfg(target_os = "macos")]
mod macos_microphone_permission;

#[cfg(target_os = "macos")]
mod macos_shortcut_monitor;

#[cfg(target_os = "macos")]
mod macos_text_delivery;

mod volcengine_asr;

#[cfg(target_os = "macos")]
pub use macos_audio_recorder::MacOsAudioRecorder;

#[cfg(target_os = "macos")]
pub use macos_feedback_sound::MacOsFeedbackSoundPlayer;

#[cfg(target_os = "macos")]
pub use macos_main_window::{MacOsMainWindowError, configure_main_window};

#[cfg(target_os = "macos")]
pub use macos_overlay_window::{MacOsOverlayWindowError, configure_overlay_window};

#[cfg(target_os = "macos")]
pub use macos_settings_store::JsonSettingsStore;

#[cfg(target_os = "macos")]
pub use macos_microphone_permission::{
    MacOsMicrophonePermission, open_microphone_privacy_settings,
};

#[cfg(target_os = "macos")]
pub use macos_shortcut_monitor::{DictationShortcutAction, MacOsShortcutMonitor};

#[cfg(target_os = "macos")]
pub use macos_text_delivery::{MacOsTextDeliverer, copy_text_to_clipboard};

pub use app_instance_guard::{AppInstanceGuard, AppInstanceGuardError};
pub use app_paths::{AppEnvironment, AppPaths};
pub use chat_completions_llm::ChatCompletionsLlmProvider;
pub use dictionary_files::{DictionaryFileError, DictionaryFileReport, DictionaryFiles};
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub use platform_secret_store::PlatformSecretStore;
pub use sqlite_storage::SqliteStorage;
pub use volcengine_asr::VolcengineSpeechRecognizer;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvRecipient {
    recipient: String,
}

impl EnvRecipient {
    pub fn from_args(mut args: impl Iterator<Item = String>) -> Self {
        let recipient = args.nth(1).unwrap_or_else(|| "world".to_owned());

        Self { recipient }
    }
}

impl RecipientProvider for EnvRecipient {
    fn recipient(&self) -> &str {
        &self.recipient
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_world_without_argument() {
        let provider = EnvRecipient::from_args(["template-cli".to_owned()].into_iter());

        assert_eq!("world", provider.recipient());
    }

    #[test]
    fn uses_first_user_argument_as_recipient() {
        let provider =
            EnvRecipient::from_args(["template-cli".to_owned(), "Rust".to_owned()].into_iter());

        assert_eq!("Rust", provider.recipient());
    }
}
