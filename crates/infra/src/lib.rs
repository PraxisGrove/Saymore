#![cfg_attr(test, allow(clippy::panic))]

mod chat_completions_llm;
mod sqlite_storage;

mod app_instance_guard;
mod app_paths;
mod cpal_audio_recorder;
mod dictation_shortcut;
mod dictionary_files;
mod model_discovery;
mod openai_transcriptions_asr;
mod storage_usage;
mod system_clock;

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod platform_secret_store;

#[cfg(target_os = "macos")]
mod macos_audio_recorder;

#[cfg(target_os = "macos")]
mod macos_application_reopen;

#[cfg(target_os = "macos")]
mod macos_application_menu;

#[cfg(target_os = "macos")]
mod macos_feedback_sound;

#[cfg(target_os = "macos")]
mod macos_dock;

#[cfg(target_os = "macos")]
mod macos_launch_at_login;

#[cfg(target_os = "macos")]
mod macos_main_window;

#[cfg(target_os = "macos")]
mod macos_overlay_window;

mod json_settings_store;

#[cfg(target_os = "windows")]
mod windows_microphone_permission;

#[cfg(target_os = "windows")]
mod windows_launch_at_login;

#[cfg(target_os = "windows")]
mod windows_feedback_sound;

#[cfg(target_os = "windows")]
mod windows_shortcut_monitor;

#[cfg(target_os = "windows")]
mod windows_shortcut_capture;

#[cfg(target_os = "windows")]
mod windows_shortcut_registry;

#[cfg(target_os = "windows")]
mod windows_right_alt_hook;

#[cfg(target_os = "windows")]
mod windows_text_delivery;

#[cfg(target_os = "windows")]
mod windows_overlay_window;

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
pub use macos_application_reopen::{MacOsApplicationReopenError, MacOsApplicationReopenHandler};

#[cfg(target_os = "macos")]
pub use macos_application_menu::{MacOsApplicationMenuError, install_macos_application_menu};

#[cfg(target_os = "macos")]
pub use macos_feedback_sound::MacOsFeedbackSoundPlayer;

#[cfg(target_os = "macos")]
pub use macos_dock::{MacOsDockError, activate_application, dock_is_visible, set_dock_visible};

#[cfg(target_os = "macos")]
pub use macos_launch_at_login::{
    LaunchAtLoginStatus, MacOsLaunchAtLoginError, launch_at_login_status, set_launch_at_login,
};

#[cfg(target_os = "macos")]
pub use macos_main_window::{MacOsMainWindowError, configure_main_window};

#[cfg(target_os = "macos")]
pub use macos_overlay_window::{MacOsOverlayWindowError, configure_overlay_window};

pub use json_settings_store::JsonSettingsStore;

#[cfg(target_os = "windows")]
pub use windows_microphone_permission::{
    WindowsMicrophonePermission, WindowsMicrophoneSettingsError,
    open_windows_microphone_privacy_settings,
};

#[cfg(target_os = "windows")]
pub use windows_launch_at_login::{WindowsLaunchAtLogin, WindowsLaunchAtLoginError};

#[cfg(target_os = "windows")]
pub use windows_feedback_sound::WindowsFeedbackSoundPlayer;

#[cfg(target_os = "windows")]
pub use windows_shortcut_monitor::{
    WindowsShortcut, WindowsShortcutController, WindowsShortcutError, WindowsShortcutMonitor,
    WindowsShortcutUpdate,
};

#[cfg(target_os = "windows")]
pub use windows_text_delivery::{WindowsTextDeliverer, copy_text_to_clipboard};

#[cfg(target_os = "windows")]
pub use windows_overlay_window::{WindowsOverlayWindowError, configure_windows_overlay_window};

#[cfg(target_os = "macos")]
pub use macos_microphone_permission::{
    MacOsMicrophonePermission, open_microphone_privacy_settings,
};

#[cfg(target_os = "macos")]
pub use macos_shortcut_monitor::{
    MacOsShortcut, MacOsShortcutController, MacOsShortcutError, MacOsShortcutMonitor,
};

#[cfg(target_os = "macos")]
pub use macos_text_delivery::{
    MacOsCorrectionObservationSupport, MacOsFocusedTextControlCapabilities, MacOsTextDeliverer,
    copy_text_to_clipboard, focused_text_control_capabilities, open_accessibility_privacy_settings,
    text_control_capabilities_for_process,
};

pub use app_instance_guard::{AppInstanceGuard, AppInstanceGuardError};
pub use app_paths::{AppEnvironment, AppPaths};
pub use chat_completions_llm::ChatCompletionsLlmProvider;
pub use cpal_audio_recorder::CpalAudioRecorder;
pub use dictation_shortcut::DictationShortcutAction;
pub use dictionary_files::{DictionaryFileError, DictionaryFileReport, DictionaryFiles};
pub use model_discovery::{ModelDiscoveryError, discover_models};
pub use openai_transcriptions_asr::OpenAiCompatibleSpeechRecognizer;
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub use platform_secret_store::PlatformSecretStore;
pub use sqlite_storage::{SqliteStorage, read_dictionary_snapshot};
pub use storage_usage::directory_usage_bytes;
pub use system_clock::SystemClock;
pub use volcengine_asr::VolcengineSpeechRecognizer;
