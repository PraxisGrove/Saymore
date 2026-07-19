#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![cfg_attr(test, allow(clippy::panic))]

use std::{
    error::Error,
    process::ExitCode,
    sync::{Arc, Mutex, atomic::AtomicBool},
};
use std::{
    sync::atomic::Ordering,
    thread,
    time::{Duration, Instant},
};

use slint::ComponentHandle;
use slint::{SharedString, Timer};
use template_app::{AudioRecorder, LocalSettingsStore};
use template_app::{
    CancelledRecordingStore, DictationSession, DictationSessionId, DictationToggleAction,
    FeedbackSound, PcmChunk, RecordingError, RecordingMetrics, RecordingStarted,
};
#[cfg(target_os = "macos")]
use template_app::{CorrectionObservingTextDeliverer, MicrophonePermissionProvider};
use template_infra::{
    AppEnvironment, AppInstanceGuard, AppPaths, DictationShortcutAction, JsonSettingsStore,
    PlatformSecretStore, SqliteStorage,
};
#[cfg(target_os = "macos")]
use template_infra::{
    MacOsApplicationReopenHandler, MacOsAudioRecorder, MacOsMicrophonePermission, MacOsShortcut,
    MacOsShortcutController, MacOsShortcutMonitor, MacOsTextDeliverer,
};

// Slint-generated code contains framework-internal unwraps and panics. Keep the
// exception scoped to generated output; handwritten production code stays strict.
#[allow(
    clippy::panic,
    clippy::todo,
    clippy::unimplemented,
    clippy::unwrap_used
)]
mod ui {
    slint::include_modules!();
}

use ui::AppWindow;
#[cfg(target_os = "macos")]
use ui::{AccessibilityPermissionOverlay, StatusTray};
use ui::{
    DictionaryAddedOverlay, MicrophoneIntroOverlay, MicrophonePermissionOverlay,
    RecordingLimitOverlay, RecordingOverlay, ResultOverlay,
};

#[cfg(target_os = "macos")]
mod accessibility_permission_prompt;
mod app_environment;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod asr_runtime;
mod authorization_ui;
#[cfg(target_os = "macos")]
mod ax_compatibility_cli;
#[cfg(target_os = "macos")]
mod ax_compatibility_server;
mod delivery_runtime;
mod desktop_core;
mod diagnostics;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod dictation_completion_runtime;
mod dictation_finish;
mod dictionary_added_overlay;
mod feedback_runtime;
mod home_stats;
mod i18n;
mod local_data_ui;
mod local_settings_runtime;
#[cfg(target_os = "macos")]
mod macos_text_delivery_runtime;
mod main_window;
mod microphone_access;
mod onboarding;
mod overlay_window;
mod permission_actions;
mod platform_open;
mod recording_actions;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod recording_limit;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod recording_metrics;
mod recording_runtime;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod recording_state;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod refinement_runtime;
mod regional_format;
mod settings_actions;
mod settings_ui;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod status_tray;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod ui_status;
mod update_check;
#[cfg(target_os = "windows")]
mod windows_runtime;
#[cfg(target_os = "windows")]
mod windows_window;

pub(crate) type RecorderHandle = Arc<Mutex<Box<dyn AudioRecorder>>>;

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn overlay_generation_matches(scheduled: i32, current: i32) -> bool {
    scheduled == current
}

#[cfg(target_os = "macos")]
use desktop_core::{PlatformAdapters, WiredCore, wire_core_services};
use dictation_completion_runtime::DictationRuntime;
use feedback_runtime::play_feedback_sound;
#[cfg(target_os = "macos")]
use macos_text_delivery_runtime::MacOsMainThreadTextDeliverer;
pub(crate) use recording_runtime::hide_overlay_after_delay;
use ui_status::*;

const CANCEL_UNDO_WINDOW: Duration = Duration::from_secs(2);
#[derive(Clone)]
pub(crate) struct DictationOverlays {
    pub(crate) status: slint::Weak<RecordingOverlay>,
    pub(crate) result: slint::Weak<ResultOverlay>,
    pub(crate) limit: slint::Weak<RecordingLimitOverlay>,
}

impl DictationOverlays {
    fn new(
        status: &RecordingOverlay,
        result: &ResultOverlay,
        limit: &RecordingLimitOverlay,
    ) -> Self {
        Self {
            status: status.as_weak(),
            result: result.as_weak(),
            limit: limit.as_weak(),
        }
    }
}

struct ShortcutRuntime {
    recorder: RecorderHandle,
    microphone_access: microphone_access::MicrophoneAccess,
    first_recording: Arc<AtomicBool>,
    session: Arc<DictationSession>,
    cancelled: Arc<Mutex<CancelledRecordingStore>>,
    paused: Arc<AtomicBool>,
    onboarding_toggle: Arc<dyn Fn() -> bool + Send + Sync>,
    #[cfg(target_os = "macos")]
    onboarding_active: Arc<dyn Fn() -> bool + Send + Sync>,
    #[cfg(target_os = "macos")]
    accessibility_permission_prompt: accessibility_permission_prompt::AccessibilityPermissionPrompt,
    dictation: DictationRuntime,
    feedback_sounds_enabled: Arc<AtomicBool>,
}

fn main() -> ExitCode {
    #[cfg(target_os = "macos")]
    if let Some(exit_code) = ax_compatibility_cli::run_if_requested() {
        return exit_code;
    }
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("failed to run Saymore: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let Some(bootstrap) = DesktopBootstrap::initialize()? else {
        return Ok(());
    };
    #[cfg(target_os = "macos")]
    return run_macos(bootstrap);
    #[cfg(target_os = "windows")]
    return windows_runtime::run(bootstrap);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    Err("Saymore desktop supports macOS and Windows".into())
}

#[cfg(target_os = "macos")]
fn run_macos(bootstrap: DesktopBootstrap) -> Result<(), Box<dyn Error>> {
    let windows = DesktopWindows::initialize(&bootstrap)?;
    let platform = macos_platform_adapters(&bootstrap);
    let core = wire_core_services(&bootstrap, &windows, platform)?;
    run_wired_desktop(bootstrap, windows, core)
}

#[cfg(target_os = "macos")]
fn macos_platform_adapters(bootstrap: &DesktopBootstrap) -> PlatformAdapters {
    let microphone: Arc<dyn MicrophonePermissionProvider> = Arc::new(MacOsMicrophonePermission);
    let deliverer: Arc<dyn CorrectionObservingTextDeliverer> = Arc::new(
        MacOsMainThreadTextDeliverer::new(Arc::new(MacOsTextDeliverer)),
    );
    let recorder: RecorderHandle = Arc::new(Mutex::new(Box::new(
        MacOsAudioRecorder::with_preferred_input_device_id(
            bootstrap.local_settings.preferred_microphone_id.clone(),
        ),
    )));
    let shortcuts = bootstrap
        .local_settings
        .dictation_shortcuts
        .iter()
        .filter_map(|value| MacOsShortcut::from_storage_value(value).ok())
        .collect();
    PlatformAdapters::new(
        recorder,
        microphone,
        deliverer,
        MacOsShortcutController::new(shortcuts),
    )
}

struct DesktopBootstrap {
    environment: AppEnvironment,
    paths: AppPaths,
    settings_store: Arc<JsonSettingsStore>,
    local_storage: Arc<SqliteStorage>,
    local_settings: template_app::LocalSettings,
    diagnostics: diagnostics::DiagnosticsController,
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    instance_guard: AppInstanceGuard,
    #[cfg(target_os = "macos")]
    _ax_compatibility_server: Option<ax_compatibility_server::AxCompatibilityServer>,
}

impl DesktopBootstrap {
    fn initialize() -> Result<Option<Self>, Box<dyn Error>> {
        let environment = app_environment::resolve()?;
        let paths = AppPaths::for_current_user(environment)?;
        let instance_guard = match AppInstanceGuard::acquire(&paths.instance_lock()) {
            Ok(guard) => guard,
            Err(template_infra::AppInstanceGuardError::AlreadyRunning) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        #[cfg(target_os = "macos")]
        let ax_compatibility_server = match environment {
            AppEnvironment::Development => Some(ax_compatibility_server::start()?),
            AppEnvironment::Production => None,
        };
        let settings_store = Arc::new(JsonSettingsStore::at_path(paths.provider_config()));
        settings_store.ensure_exists()?;
        let local_storage = open_local_storage(&paths, environment)?;
        let local_settings = load_local_settings(&local_storage);
        let diagnostics =
            initialize_diagnostics(&paths, local_settings.diagnostics_logging_enabled);
        Ok(Some(Self {
            environment,
            paths,
            settings_store,
            local_storage,
            local_settings,
            diagnostics,
            instance_guard,
            #[cfg(target_os = "macos")]
            _ax_compatibility_server: ax_compatibility_server,
        }))
    }
}

struct DesktopWindows {
    ui: AppWindow,
    overlay: RecordingOverlay,
    result_overlay: ResultOverlay,
    recording_limit_overlay: RecordingLimitOverlay,
    dictionary_added_overlay: DictionaryAddedOverlay,
    microphone_intro_overlay: MicrophoneIntroOverlay,
    microphone_permission_overlay: MicrophonePermissionOverlay,
    #[cfg(target_os = "macos")]
    accessibility_permission_overlay: AccessibilityPermissionOverlay,
    language_context: i18n::LanguageContext,
    #[cfg(target_os = "macos")]
    _reopen_handler: Option<MacOsApplicationReopenHandler>,
}

impl DesktopWindows {
    fn initialize(bootstrap: &DesktopBootstrap) -> Result<Self, Box<dyn Error>> {
        let ui = AppWindow::new()?;
        #[cfg(target_os = "macos")]
        let reopen_handler = install_application_reopen_handler(&ui)?;
        let language_context =
            main_window::initialize(&ui, &bootstrap.local_settings, bootstrap.environment)?;
        Ok(Self {
            ui,
            overlay: RecordingOverlay::new()?,
            result_overlay: ResultOverlay::new()?,
            recording_limit_overlay: RecordingLimitOverlay::new()?,
            dictionary_added_overlay: DictionaryAddedOverlay::new()?,
            microphone_intro_overlay: MicrophoneIntroOverlay::new()?,
            microphone_permission_overlay: MicrophonePermissionOverlay::new()?,
            #[cfg(target_os = "macos")]
            accessibility_permission_overlay: AccessibilityPermissionOverlay::new()?,
            language_context,
            #[cfg(target_os = "macos")]
            _reopen_handler: reopen_handler,
        })
    }
}

#[cfg(target_os = "macos")]
fn run_wired_desktop(
    bootstrap: DesktopBootstrap,
    windows: DesktopWindows,
    core: WiredCore,
) -> Result<(), Box<dyn Error>> {
    let first_recording = Arc::new(AtomicBool::new(true));
    let paused = Arc::new(AtomicBool::new(bootstrap.local_settings.dictation_paused));
    delivery_runtime::wire_result_actions(&windows.result_overlay);
    dictionary_added_overlay::wire(&windows.ui, &windows.dictionary_added_overlay);
    let dismiss_limit = windows.recording_limit_overlay.as_weak();
    windows.recording_limit_overlay.on_acknowledged(move || {
        if let Some(overlay) = dismiss_limit.upgrade() {
            let _ = overlay.hide();
        }
    });
    let pause_recording = recording_actions::wire(
        &windows.ui,
        &windows.overlay,
        &windows.result_overlay,
        &windows.recording_limit_overlay,
        recording_actions::RecordingActionRuntime {
            recorder: Arc::clone(&core.recorder),
            session: Arc::clone(&core.session),
            cancelled: Arc::clone(&core.cancelled),
            dictation: core.dictation.clone(),
            feedback_sounds_enabled: Arc::clone(&core.feedback_sounds_enabled),
        },
    );
    let overlays = DictationOverlays::new(
        &windows.overlay,
        &windows.result_overlay,
        &windows.recording_limit_overlay,
    );
    let onboarding_shortcut = core.onboarding.shortcut_handler();
    let permission_onboarding_shortcut = onboarding_shortcut.clone();
    let onboarding_toggle = Arc::new(move || onboarding_shortcut.handle_toggle());
    let onboarding_active = Arc::new(move || permission_onboarding_shortcut.is_active());
    let accessibility_permission_prompt =
        accessibility_permission_prompt::wire(&windows.accessibility_permission_overlay);
    let _shortcut_monitor = recording_runtime::start_recording_shortcut(
        &windows.ui,
        overlays,
        core.shortcut_controller,
        ShortcutRuntime {
            recorder: core.recorder,
            microphone_access: core.microphone_access,
            first_recording,
            session: core.session,
            cancelled: core.cancelled,
            paused: Arc::clone(&paused),
            onboarding_toggle,
            onboarding_active,
            accessibility_permission_prompt,
            dictation: core.dictation,
            feedback_sounds_enabled: Arc::clone(&core.feedback_sounds_enabled),
        },
    )
    .map_err(std::io::Error::other)?;
    let tray = StatusTray::new()?;
    status_tray::wire(
        &tray,
        &windows.ui,
        core.local_settings.clone(),
        paused,
        pause_recording,
    );
    prepare_overlay_windows([
        windows.overlay.window(),
        windows.result_overlay.window(),
        windows.recording_limit_overlay.window(),
        windows.dictionary_added_overlay.window(),
        windows.microphone_intro_overlay.window(),
        windows.microphone_permission_overlay.window(),
        windows.accessibility_permission_overlay.window(),
    ]);
    run_desktop_event_loop(&windows.ui, &tray, &core.onboarding)?;
    drop(core.authorization_poll);
    drop(core.feedback_sounds_enabled);
    Ok(())
}

fn prepare_overlay_windows<const N: usize>(windows: [&slint::Window; N]) {
    for window in windows {
        recording_runtime::prepare_overlay_window(window);
    }
}

fn open_local_storage(
    paths: &AppPaths,
    environment: AppEnvironment,
) -> Result<Arc<SqliteStorage>, Box<dyn Error>> {
    let secrets = Arc::new(PlatformSecretStore::new(environment)?);
    Ok(Arc::new(SqliteStorage::start(paths.database(), secrets)?))
}

#[cfg(target_os = "macos")]
fn install_application_reopen_handler(
    ui: &AppWindow,
) -> Result<Option<MacOsApplicationReopenHandler>, template_infra::MacOsApplicationReopenError> {
    let reopen_ui = ui.as_weak();
    match MacOsApplicationReopenHandler::install(move || status_tray::show_window(&reopen_ui, None))
    {
        Ok(handler) => Ok(Some(handler)),
        Err(template_infra::MacOsApplicationReopenError::AlreadyInstalled) => {
            tracing::info!(
                event = "application.reopen_handler_skipped",
                reason = "the desktop backend already owns the application delegate"
            );
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn initialize_diagnostics(paths: &AppPaths, enabled: bool) -> diagnostics::DiagnosticsController {
    let directory = paths.data_directory().join("logs");
    diagnostics::init(directory.clone(), enabled).unwrap_or_else(|error| {
        eprintln!("failed to initialize local diagnostics: {error}");
        diagnostics::DiagnosticsController::without_logger(directory, enabled)
    })
}

fn load_local_settings(storage: &SqliteStorage) -> template_app::LocalSettings {
    storage.load_settings().unwrap_or_else(|error| {
        eprintln!("failed to load local settings: {error}");
        template_app::LocalSettings::default()
    })
}

#[cfg(target_os = "macos")]
fn run_desktop_event_loop(
    ui: &AppWindow,
    tray: &StatusTray,
    onboarding: &onboarding::OnboardingRuntime,
) -> Result<(), slint::PlatformError> {
    slint::invoke_from_event_loop(|| {
        if let Err(error) = template_infra::install_macos_application_menu() {
            tracing::error!(event = "application.menu_install_failed", reason = %error);
        }
    })
    .map_err(|error| slint::PlatformError::Other(error.to_string()))?;
    tray.show()?;
    onboarding.present_initial(ui)?;
    slint::run_event_loop_until_quit()?;
    tray.hide()?;
    onboarding.hide();
    ui.hide()?;
    Ok(())
}

fn prewarm_audio_recorder(recorder: &RecorderHandle) {
    let recorder = Arc::clone(recorder);
    let _ = thread::Builder::new()
        .name("saymore-audio-prewarm".to_owned())
        .spawn(move || match recorder.lock() {
            Ok(mut recorder) => {
                if let Err(error) = recorder.prepare()
                    && error != RecordingError::PermissionDenied
                {
                    tracing::warn!(event = "recording.audio_preload_failed", reason = %error);
                }
            }
            Err(_) => tracing::warn!(
                event = "recording.audio_preload_failed",
                reason = "recorder lock was poisoned"
            ),
        });
}
