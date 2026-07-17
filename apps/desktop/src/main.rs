#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![cfg_attr(test, allow(clippy::panic))]

use std::{error::Error, process::ExitCode};
#[cfg(target_os = "macos")]
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use slint::ComponentHandle;
#[cfg(target_os = "macos")]
use slint::{SharedString, Timer};
#[cfg(target_os = "macos")]
use template_app::{
    AudioRecorder, CancelledRecordingStore, DictationSession, DictationSessionId,
    DictationToggleAction, LocalSettingsStore, MicrophonePermissionProvider, PcmChunk,
    RecordingError, RecordingMetrics, RecordingStarted,
};
#[cfg(target_os = "macos")]
use template_app::{FeedbackSound, TextDeliverer};
#[cfg(target_os = "macos")]
use template_infra::{
    AppEnvironment, AppInstanceGuard, AppPaths, DictationShortcutAction, JsonSettingsStore,
    MacOsApplicationReopenHandler, MacOsAudioRecorder, MacOsMicrophonePermission, MacOsShortcut,
    MacOsShortcutController, MacOsShortcutMonitor, MacOsTextDeliverer, PlatformSecretStore,
    SqliteStorage,
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

use ui::{AppWindow, Translations};
#[cfg(target_os = "macos")]
use ui::{
    DictionaryAddedOverlay, MicrophoneIntroOverlay, MicrophonePermissionOverlay,
    RecordingLimitOverlay, RecordingOverlay, ResultOverlay, StatusTray,
};

#[cfg(target_os = "macos")]
mod app_environment;
#[cfg(target_os = "macos")]
mod asr_runtime;
#[cfg(target_os = "macos")]
mod authorization_ui;
#[cfg(target_os = "macos")]
mod ax_compatibility_cli;
#[cfg(target_os = "macos")]
mod ax_compatibility_server;
#[cfg(target_os = "macos")]
mod delivery_runtime;
#[cfg(target_os = "macos")]
mod diagnostics;
#[cfg(target_os = "macos")]
mod dictation_completion_runtime;
#[cfg(target_os = "macos")]
mod dictation_finish;
#[cfg(target_os = "macos")]
mod dictionary_added_overlay;
#[cfg(target_os = "macos")]
mod feedback_runtime;
#[cfg(target_os = "macos")]
mod home_stats;
#[cfg(target_os = "macos")]
mod i18n;
#[cfg(target_os = "macos")]
mod local_data_ui;
#[cfg(target_os = "macos")]
mod main_window;
#[cfg(target_os = "macos")]
mod microphone_access;
#[cfg(target_os = "macos")]
mod onboarding;
#[cfg(target_os = "macos")]
mod overlay_window;
#[cfg(target_os = "macos")]
mod recording_actions;
#[cfg(target_os = "macos")]
mod recording_limit;
#[cfg(target_os = "macos")]
mod recording_metrics;
#[cfg(target_os = "macos")]
mod recording_runtime;
#[cfg(target_os = "macos")]
mod recording_state;
#[cfg(target_os = "macos")]
mod refinement_runtime;
#[cfg(target_os = "macos")]
mod regional_format;
#[cfg(target_os = "macos")]
mod settings_actions;
#[cfg(target_os = "macos")]
mod settings_ui;
#[cfg(target_os = "macos")]
mod status_tray;
#[cfg(target_os = "macos")]
mod ui_status;
#[cfg(target_os = "macos")]
mod update_check;

#[cfg(target_os = "macos")]
use dictation_completion_runtime::DictationRuntime;
#[cfg(target_os = "macos")]
use feedback_runtime::{initialize as initialize_feedback_sounds, play_feedback_sound};
#[cfg(target_os = "macos")]
pub(crate) use recording_runtime::{hide_overlay_after_delay, overlay_generation_matches};
#[cfg(target_os = "macos")]
use ui_status::*;

#[cfg(target_os = "macos")]
const CANCEL_UNDO_WINDOW: Duration = Duration::from_secs(2);
#[cfg(target_os = "macos")]
#[derive(Clone)]
pub(crate) struct DictationOverlays {
    pub(crate) status: slint::Weak<RecordingOverlay>,
    pub(crate) result: slint::Weak<ResultOverlay>,
    pub(crate) limit: slint::Weak<RecordingLimitOverlay>,
}

#[cfg(target_os = "macos")]
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

#[cfg(target_os = "macos")]
struct ShortcutRuntime {
    recorder: Arc<Mutex<MacOsAudioRecorder>>,
    microphone_access: microphone_access::MicrophoneAccess,
    first_recording: Arc<AtomicBool>,
    session: Arc<DictationSession>,
    cancelled: Arc<Mutex<CancelledRecordingStore>>,
    paused: Arc<AtomicBool>,
    onboarding: onboarding::OnboardingShortcutHandler,
    dictation: DictationRuntime,
    feedback_sounds_enabled: Arc<AtomicBool>,
}

#[cfg(target_os = "macos")]
struct LocalFeatureOptions {
    data_directory: std::path::PathBuf,
    automatic_update_checks: bool,
    shortcut_controller: MacOsShortcutController,
}

#[cfg(target_os = "macos")]
fn create_dictation_runtime(
    settings_store: &Arc<JsonSettingsStore>,
    local_storage: &Arc<SqliteStorage>,
    deliverer: MacOsTextDeliverer,
) -> Result<DictationRuntime, Box<dyn Error>> {
    Ok(DictationRuntime::new(
        Arc::clone(settings_store),
        Arc::clone(local_storage),
        Arc::new(deliverer),
    )?)
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

#[cfg(target_os = "macos")]
fn run() -> Result<(), Box<dyn Error>> {
    let bootstrap = DesktopBootstrap::initialize()?;
    let windows = DesktopWindows::initialize(&bootstrap)?;
    let deliverer = MacOsTextDeliverer;
    let microphone = MacOsMicrophonePermission;
    let core = wire_core_services(&bootstrap, &windows, deliverer, microphone)?;
    run_wired_desktop(bootstrap, windows, core)
}

#[cfg(target_os = "macos")]
struct DesktopBootstrap {
    environment: AppEnvironment,
    paths: AppPaths,
    settings_store: Arc<JsonSettingsStore>,
    local_storage: Arc<SqliteStorage>,
    local_settings: template_app::LocalSettings,
    shortcut_controller: MacOsShortcutController,
    diagnostics: diagnostics::DiagnosticsController,
    _instance_guard: AppInstanceGuard,
    _ax_compatibility_server: Option<ax_compatibility_server::AxCompatibilityServer>,
}

#[cfg(target_os = "macos")]
impl DesktopBootstrap {
    fn initialize() -> Result<Self, Box<dyn Error>> {
        let environment = app_environment::resolve()?;
        let paths = AppPaths::for_current_user(environment)?;
        let instance_guard = AppInstanceGuard::acquire(&paths.instance_lock())?;
        let ax_compatibility_server = match environment {
            AppEnvironment::Development => Some(ax_compatibility_server::start()?),
            AppEnvironment::Production => None,
        };
        let settings_store = Arc::new(JsonSettingsStore::at_path(paths.provider_config()));
        settings_store.ensure_exists()?;
        let local_storage = open_local_storage(&paths, environment)?;
        let local_settings = load_local_settings(&local_storage);
        let shortcuts = local_settings
            .dictation_shortcuts
            .iter()
            .filter_map(|value| MacOsShortcut::from_storage_value(value).ok())
            .collect();
        let shortcut_controller = MacOsShortcutController::new(shortcuts);
        let diagnostics =
            initialize_diagnostics(&paths, local_settings.diagnostics_logging_enabled);
        Ok(Self {
            environment,
            paths,
            settings_store,
            local_storage,
            local_settings,
            shortcut_controller,
            diagnostics,
            _instance_guard: instance_guard,
            _ax_compatibility_server: ax_compatibility_server,
        })
    }
}

#[cfg(target_os = "macos")]
struct DesktopWindows {
    ui: AppWindow,
    overlay: RecordingOverlay,
    result_overlay: ResultOverlay,
    recording_limit_overlay: RecordingLimitOverlay,
    dictionary_added_overlay: DictionaryAddedOverlay,
    microphone_intro_overlay: MicrophoneIntroOverlay,
    microphone_permission_overlay: MicrophonePermissionOverlay,
    language_context: i18n::LanguageContext,
    _reopen_handler: Option<MacOsApplicationReopenHandler>,
}

#[cfg(target_os = "macos")]
impl DesktopWindows {
    fn initialize(bootstrap: &DesktopBootstrap) -> Result<Self, Box<dyn Error>> {
        let ui = AppWindow::new()?;
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
            language_context,
            _reopen_handler: reopen_handler,
        })
    }
}

#[cfg(target_os = "macos")]
struct WiredCore {
    recorder: Arc<Mutex<MacOsAudioRecorder>>,
    session: Arc<DictationSession>,
    cancelled: Arc<Mutex<CancelledRecordingStore>>,
    feedback_sounds_enabled: Arc<AtomicBool>,
    dictation: DictationRuntime,
    microphone_access: microphone_access::MicrophoneAccess,
    onboarding: onboarding::OnboardingRuntime,
    authorization_poll: authorization_ui::AuthorizationPoll,
    local_settings_guard: Arc<Mutex<()>>,
}

#[cfg(target_os = "macos")]
fn wire_core_services(
    bootstrap: &DesktopBootstrap,
    windows: &DesktopWindows,
    deliverer: MacOsTextDeliverer,
    microphone: MacOsMicrophonePermission,
) -> Result<WiredCore, Box<dyn Error>> {
    let recorder = Arc::new(Mutex::new(
        MacOsAudioRecorder::with_preferred_input_device_id(
            bootstrap.local_settings.preferred_microphone_id.clone(),
        ),
    ));
    prewarm_audio_recorder(&recorder);
    let (session, cancelled) = recording_state::initialize(CANCEL_UNDO_WINDOW);
    let feedback_sounds_enabled =
        initialize_feedback_sounds(bootstrap.local_settings.feedback_sounds_enabled);
    let dictation = create_dictation_runtime(
        &bootstrap.settings_store,
        &bootstrap.local_storage,
        deliverer,
    )?;
    update_authorizations(
        &windows.ui,
        deliverer.authorization(),
        microphone.authorization(),
    );
    let microphone_access = microphone_access::wire(
        &windows.ui,
        &windows.microphone_intro_overlay,
        &windows.microphone_permission_overlay,
        microphone,
    );
    settings_ui::wire(&windows.ui, Arc::clone(&bootstrap.settings_store));
    let local_settings_guard = Arc::new(Mutex::new(()));
    i18n::wire(
        &windows.ui,
        Arc::clone(&bootstrap.local_storage),
        Arc::clone(&local_settings_guard),
        windows.language_context,
    );
    wire_local_features(
        &windows.ui,
        Arc::clone(&bootstrap.local_storage),
        Arc::clone(&recorder),
        Arc::clone(&local_settings_guard),
        Arc::clone(&feedback_sounds_enabled),
        bootstrap.diagnostics.clone(),
        LocalFeatureOptions {
            data_directory: bootstrap.paths.data_directory().to_path_buf(),
            automatic_update_checks: bootstrap.local_settings.automatic_update_checks,
            shortcut_controller: bootstrap.shortcut_controller.clone(),
        },
    );
    let onboarding = onboarding::OnboardingRuntime::new(
        &windows.ui,
        &bootstrap.local_settings,
        Arc::clone(&bootstrap.local_storage),
        Arc::clone(&local_settings_guard),
        Arc::clone(&recorder),
        microphone,
        deliverer,
    )?;
    let authorization_poll = authorization_ui::wire(&windows.ui, deliverer, microphone);
    Ok(WiredCore {
        recorder,
        session,
        cancelled,
        feedback_sounds_enabled,
        dictation,
        microphone_access,
        onboarding,
        authorization_poll,
        local_settings_guard,
    })
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
    recording_runtime::start_recording_shortcut(
        &windows.ui,
        overlays,
        bootstrap.shortcut_controller,
        ShortcutRuntime {
            recorder: core.recorder,
            microphone_access: core.microphone_access,
            first_recording,
            session: core.session,
            cancelled: core.cancelled,
            paused: Arc::clone(&paused),
            onboarding: core.onboarding.shortcut_handler(),
            dictation: core.dictation,
            feedback_sounds_enabled: Arc::clone(&core.feedback_sounds_enabled),
        },
    );
    let tray = StatusTray::new()?;
    status_tray::wire(
        &tray,
        &windows.ui,
        Arc::clone(&bootstrap.local_storage),
        Arc::clone(&core.local_settings_guard),
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
    ]);
    run_desktop_event_loop(
        &windows.ui,
        &tray,
        &core.onboarding,
        &bootstrap.local_storage,
        &core.local_settings_guard,
    )?;
    drop(core.authorization_poll);
    drop(core.feedback_sounds_enabled);
    Ok(())
}

#[cfg(target_os = "macos")]
fn prepare_overlay_windows<const N: usize>(windows: [&slint::Window; N]) {
    for window in windows {
        recording_runtime::prepare_overlay_window(window);
    }
}

#[cfg(target_os = "macos")]
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

#[cfg(target_os = "macos")]
fn initialize_diagnostics(paths: &AppPaths, enabled: bool) -> diagnostics::DiagnosticsController {
    let directory = paths.data_directory().join("logs");
    diagnostics::init(directory.clone(), enabled).unwrap_or_else(|error| {
        eprintln!("failed to initialize local diagnostics: {error}");
        diagnostics::DiagnosticsController::without_logger(directory, enabled)
    })
}

#[cfg(target_os = "macos")]
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
    storage: &SqliteStorage,
    settings_guard: &Mutex<()>,
) -> Result<(), slint::PlatformError> {
    tray.show()?;
    onboarding.present_initial(ui, storage, settings_guard)?;
    slint::run_event_loop_until_quit()?;
    tray.hide()?;
    onboarding.hide();
    ui.hide()?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn prewarm_audio_recorder(recorder: &Arc<Mutex<MacOsAudioRecorder>>) {
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

#[cfg(target_os = "macos")]
fn wire_local_features(
    ui: &AppWindow,
    storage: Arc<SqliteStorage>,
    recorder: Arc<Mutex<MacOsAudioRecorder>>,
    settings_guard: Arc<Mutex<()>>,
    feedback_sounds_enabled: Arc<AtomicBool>,
    diagnostics: diagnostics::DiagnosticsController,
    options: LocalFeatureOptions,
) {
    home_stats::wire(ui, Arc::clone(&storage), options.data_directory.clone());
    local_data_ui::wire(
        ui,
        Arc::clone(&storage),
        recorder,
        Arc::clone(&settings_guard),
    );
    update_check::wire(ui);
    settings_actions::wire(
        ui,
        storage,
        settings_guard,
        feedback_sounds_enabled,
        diagnostics,
        options.data_directory,
        options.shortcut_controller,
    );
    if options.automatic_update_checks {
        ui.invoke_check_for_updates();
    }
}

#[cfg(not(target_os = "macos"))]
fn run() -> Result<(), Box<dyn Error>> {
    let ui = AppWindow::new()?;
    let unsupported = ui
        .global::<Translations>()
        .get_common_not_supported_platform();
    ui.set_authorization_status(unsupported.clone());
    ui.set_microphone_status(unsupported);
    ui.run()?;
    Ok(())
}
