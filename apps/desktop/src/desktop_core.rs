use std::{
    error::Error,
    sync::{Arc, Mutex, atomic::AtomicBool},
};

use template_app::{
    CancelledRecordingStore, CorrectionObservingTextDeliverer, DictationSession,
    LocalSettingsMutator, LocalSettingsStore, MicrophonePermissionProvider, TextDeliverer,
};

use crate::{
    DesktopBootstrap, DesktopWindows, DictationRuntime, RecorderHandle, authorization_ui,
    feedback_runtime, home_stats, i18n, local_data_ui, local_settings_runtime, microphone_access,
    prewarm_audio_recorder, settings_actions, settings_ui, update_authorizations, update_check,
};

pub(crate) struct PlatformAdapters {
    recorder: RecorderHandle,
    microphone: Arc<dyn MicrophonePermissionProvider>,
    deliverer: Arc<dyn CorrectionObservingTextDeliverer>,
    shortcut_controller: settings_actions::PlatformShortcutController,
}

impl PlatformAdapters {
    pub(crate) fn new(
        recorder: RecorderHandle,
        microphone: Arc<dyn MicrophonePermissionProvider>,
        deliverer: Arc<dyn CorrectionObservingTextDeliverer>,
        shortcut_controller: settings_actions::PlatformShortcutController,
    ) -> Self {
        Self {
            recorder,
            microphone,
            deliverer,
            shortcut_controller,
        }
    }
}

pub(crate) struct WiredCore {
    pub(crate) recorder: RecorderHandle,
    pub(crate) session: Arc<DictationSession>,
    pub(crate) cancelled: Arc<Mutex<CancelledRecordingStore>>,
    pub(crate) feedback_sounds_enabled: Arc<AtomicBool>,
    pub(crate) dictation: DictationRuntime,
    pub(crate) microphone_access: microphone_access::MicrophoneAccess,
    pub(crate) shortcut_controller: settings_actions::PlatformShortcutController,
    pub(crate) onboarding: crate::onboarding::OnboardingRuntime,
    pub(crate) authorization_poll: authorization_ui::AuthorizationPoll,
    pub(crate) local_settings: local_settings_runtime::LocalSettingsHandle,
    pub(crate) _local_settings_runtime: local_settings_runtime::LocalSettingsRuntime,
}

pub(crate) fn wire_core_services(
    bootstrap: &DesktopBootstrap,
    windows: &DesktopWindows,
    platform: PlatformAdapters,
) -> Result<WiredCore, Box<dyn Error>> {
    let PlatformAdapters {
        recorder,
        microphone,
        deliverer,
        shortcut_controller,
    } = platform;
    prewarm_audio_recorder(&recorder);
    let (session, cancelled) = crate::recording_state::initialize(crate::CANCEL_UNDO_WINDOW);
    let feedback_sounds_enabled =
        feedback_runtime::initialize(bootstrap.local_settings.feedback_sounds_enabled);
    let dictation = DictationRuntime::new(
        Arc::clone(&bootstrap.settings_store),
        Arc::clone(&bootstrap.local_storage),
        Arc::clone(&deliverer),
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
        Arc::clone(&microphone),
    );
    settings_ui::wire(&windows.ui, Arc::clone(&bootstrap.settings_store));
    let settings_store: Arc<dyn LocalSettingsStore> = bootstrap.local_storage.clone();
    let local_settings_runtime = local_settings_runtime::LocalSettingsRuntime::new(Arc::new(
        LocalSettingsMutator::new(settings_store),
    ))?;
    let local_settings = local_settings_runtime.handle();
    i18n::wire(
        &windows.ui,
        local_settings.clone(),
        windows.language_context,
    );
    wire_local_features(
        bootstrap,
        windows,
        Arc::clone(&recorder),
        local_settings.clone(),
        Arc::clone(&feedback_sounds_enabled),
        shortcut_controller.clone(),
    );
    let onboarding_deliverer: Arc<dyn TextDeliverer> = deliverer.clone();
    let onboarding = crate::onboarding::OnboardingRuntime::new(
        &windows.ui,
        &bootstrap.local_settings,
        bootstrap.environment,
        local_settings.clone(),
        Arc::clone(&recorder),
        Arc::clone(&microphone),
        onboarding_deliverer,
    )?;
    let authorization_deliverer: Arc<dyn TextDeliverer> = deliverer;
    let authorization_poll =
        authorization_ui::wire(&windows.ui, authorization_deliverer, microphone);
    Ok(WiredCore {
        recorder,
        session,
        cancelled,
        feedback_sounds_enabled,
        dictation,
        microphone_access,
        shortcut_controller,
        onboarding,
        authorization_poll,
        local_settings,
        _local_settings_runtime: local_settings_runtime,
    })
}

fn wire_local_features(
    bootstrap: &DesktopBootstrap,
    windows: &DesktopWindows,
    recorder: RecorderHandle,
    settings: local_settings_runtime::LocalSettingsHandle,
    feedback_sounds_enabled: Arc<AtomicBool>,
    shortcut_controller: settings_actions::PlatformShortcutController,
) {
    let data_directory = bootstrap.paths.data_directory().to_path_buf();
    home_stats::wire(
        &windows.ui,
        Arc::clone(&bootstrap.local_storage),
        data_directory.clone(),
    );
    local_data_ui::wire(
        &windows.ui,
        Arc::clone(&bootstrap.local_storage),
        recorder,
        settings.clone(),
    );
    update_check::wire(&windows.ui);
    settings_actions::wire(
        &windows.ui,
        Arc::clone(&bootstrap.local_storage),
        settings,
        feedback_sounds_enabled,
        bootstrap.diagnostics.clone(),
        settings_actions::PlatformOptions {
            data_directory,
            shortcut_controller,
            environment: bootstrap.environment,
        },
    );
    if bootstrap.local_settings.automatic_update_checks {
        windows.ui.invoke_check_for_updates();
    }
}
