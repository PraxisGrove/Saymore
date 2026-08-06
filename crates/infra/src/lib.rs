#![cfg_attr(test, allow(clippy::panic))]

mod chat_completions_llm;
mod sqlite_storage;

mod app_instance_guard;
mod app_paths;
mod cpal_audio_recorder;
mod dictation_shortcut;
mod dictionary_files;
mod local_inference_device;
mod model_discovery;
mod model_installer;
mod offline_punctuation;
mod openai_transcriptions_asr;
mod paraformer_asr;
mod process_memory;
mod qwen3_asr;
mod sense_voice_asr;
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

#[cfg(target_os = "macos")]
mod macos_system_audio;

#[cfg(target_os = "macos")]
mod macos_speech_probe;

#[cfg(target_os = "macos")]
mod macos_speech;

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

#[cfg(target_os = "windows")]
mod windows_system_audio;

#[cfg(target_os = "macos")]
mod macos_microphone_permission;

#[cfg(target_os = "macos")]
mod macos_shortcut_monitor;

#[cfg(target_os = "macos")]
mod macos_text_delivery;

mod volcengine_asr;
mod whisper_asr;

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

#[cfg(target_os = "macos")]
pub use macos_system_audio::MacOsOutputAudioMuter;

#[cfg(target_os = "macos")]
pub use macos_speech_probe::{
    MacOsSpeechProbeError, MacOsSpeechProbeReport, run_macos_speech_probe,
};

#[cfg(target_os = "macos")]
pub use macos_speech::{
    MacOsSpeechAuthorization, MacOsSpeechCapability, MacOsSpeechRecognizer, macos_product_version,
    macos_speech_capability, open_speech_recognition_privacy_settings,
    request_macos_speech_authorization,
};

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

#[cfg(target_os = "windows")]
pub use windows_system_audio::WindowsOutputAudioMuter;

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
    MacOsTextDeliveryProgress, MacOsTextDeliverySession, copy_text_to_clipboard,
    focused_text_control_capabilities, open_accessibility_privacy_settings,
    text_control_capabilities_for_process,
};

pub use app_instance_guard::{AppInstanceGuard, AppInstanceGuardError};
pub use app_paths::{AppEnvironment, AppPaths};
pub use chat_completions_llm::ChatCompletionsLlmProvider;
pub use cpal_audio_recorder::CpalAudioRecorder;
pub use dictation_shortcut::DictationShortcutAction;
pub use dictionary_files::{DictionaryFileError, DictionaryFileReport, DictionaryFiles};
pub use local_inference_device::{LocalInferenceDevice, local_inference_device};
pub use model_discovery::{ModelDiscoveryError, discover_models};
pub use model_installer::{
    ModelDownloadProgress, ModelInstallControl, ModelInstallError, ModelInstallInterruption,
    PARAFORMER_MODEL_ID, PARAFORMER_MODEL_REVISION, PUNCTUATION_MODEL_ID,
    PUNCTUATION_MODEL_REVISION, QWEN3_ASR_MODEL_ID, QWEN3_ASR_MODEL_REVISION, SENSE_VOICE_MODEL_ID,
    SENSE_VOICE_MODEL_REVISION, VerifiedModelInstaller, WHISPER_MODEL_ID, WHISPER_MODEL_REVISION,
};
pub use offline_punctuation::OfflinePunctuationRestorer;
pub use openai_transcriptions_asr::OpenAiCompatibleSpeechRecognizer;
pub use paraformer_asr::ParaformerSpeechRecognizer;
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub use platform_secret_store::PlatformSecretStore;
pub use process_memory::current_process_resident_memory_bytes;
pub use qwen3_asr::Qwen3AsrSpeechRecognizer;
pub use sense_voice_asr::SenseVoiceSpeechRecognizer;
pub use sqlite_storage::{SqliteStorage, read_dictionary_snapshot};
pub use storage_usage::{directory_usage_bytes, local_storage_usage};
pub use system_clock::SystemClock;
pub use volcengine_asr::VolcengineSpeechRecognizer;
pub use whisper_asr::WhisperSpeechRecognizer;
