use std::{
    error::Error,
    sync::{Arc, Mutex, atomic::AtomicBool},
};

use slint::ComponentHandle;
use template_app::{CorrectionObservingTextDeliverer, MicrophonePermissionProvider};
use template_infra::{
    CpalAudioRecorder, WindowsMicrophonePermission, WindowsOutputAudioMuter, WindowsShortcut,
    WindowsShortcutController, WindowsTextDeliverer,
};

use crate::{
    DesktopBootstrap, DesktopWindows, DictationOverlays, RecorderHandle, ShortcutRuntime,
    delivery_runtime,
    desktop_core::{PlatformAdapters, WiredCore, wire_core_services},
    dictionary_added_overlay, prepare_overlay_windows, recording_actions, recording_runtime,
    status_tray,
    ui::StatusTray,
};

pub(crate) fn run(mut bootstrap: DesktopBootstrap) -> Result<(), Box<dyn Error>> {
    let windows = DesktopWindows::initialize(&bootstrap)?;
    let activation_ui = windows.ui.as_weak();
    bootstrap.instance_guard.listen_for_activation(move || {
        let activation_ui = activation_ui.clone();
        let show_ui = activation_ui.clone();
        let _ = activation_ui.upgrade_in_event_loop(move |_| {
            status_tray::show_window(&show_ui, None);
        });
    })?;
    let platform = windows_platform_adapters(&bootstrap)?;
    let core = wire_core_services(&bootstrap, &windows, platform)?;
    let (_shortcut_monitor, tray) = wire_recording(&bootstrap, &windows, &core)?;
    tray.show()?;
    if crate::app_environment::started_automatically() {
        slint::run_event_loop_until_quit()?;
    } else {
        core.onboarding.present_initial(&windows.ui)?;
        slint::run_event_loop_until_quit()?;
        core.onboarding.hide();
    }
    tray.hide()?;
    windows.ui.hide()?;
    drop(core.authorization_poll);
    Ok(())
}

fn windows_platform_adapters(
    bootstrap: &DesktopBootstrap,
) -> Result<PlatformAdapters, Box<dyn Error>> {
    let microphone: Arc<dyn MicrophonePermissionProvider> = Arc::new(WindowsMicrophonePermission);
    let deliverer: Arc<dyn CorrectionObservingTextDeliverer> =
        Arc::new(WindowsTextDeliverer::new()?);
    let recorder: RecorderHandle =
        Arc::new(Mutex::new(crate::recording_audio::RecordingAudio::new(
            Box::new(CpalAudioRecorder::new(
                Arc::clone(&microphone),
                bootstrap.local_settings.preferred_microphone_id.clone(),
            )),
            Arc::new(WindowsOutputAudioMuter),
        )));
    let stored_shortcuts = &bootstrap.local_settings.dictation_shortcuts;
    let mut shortcuts: Vec<_> = stored_shortcuts
        .iter()
        .filter_map(|value| WindowsShortcut::from_storage_value(value).ok())
        .collect();
    if shortcuts.is_empty() && !stored_shortcuts.is_empty() {
        shortcuts.push(WindowsShortcut::default());
    }
    let shortcut_controller = WindowsShortcutController::new(shortcuts);
    Ok(PlatformAdapters::new(
        recorder,
        microphone,
        deliverer,
        shortcut_controller,
    ))
}

fn wire_recording(
    bootstrap: &DesktopBootstrap,
    windows: &DesktopWindows,
    core: &WiredCore,
) -> Result<(recording_runtime::PlatformShortcutMonitor, StatusTray), Box<dyn Error>> {
    delivery_runtime::wire_result_actions(&windows.result_overlay);
    dictionary_added_overlay::wire(&windows.ui, &windows.dictionary_added_overlay);
    crate::asr_configuration_prompt::wire(&windows.ui, &windows.asr_configuration_overlay);
    let dismiss_limit = windows.recording_limit_overlay.as_weak();
    windows.recording_limit_overlay.on_acknowledged(move || {
        if let Some(overlay) = dismiss_limit.upgrade() {
            let _ = overlay.hide();
        }
    });
    let paused = Arc::new(AtomicBool::new(bootstrap.local_settings.dictation_paused));
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
    prepare_overlay_windows([
        windows.overlay.window(),
        windows.result_overlay.window(),
        windows.recording_limit_overlay.window(),
        windows.dictionary_added_overlay.window(),
        windows.microphone_intro_overlay.window(),
        windows.microphone_permission_overlay.window(),
        windows.asr_configuration_overlay.window(),
    ]);
    let overlays = DictationOverlays::new(
        &windows.overlay,
        &windows.result_overlay,
        &windows.recording_limit_overlay,
    )
    .with_asr_configuration(&windows.asr_configuration_overlay);
    let shortcut_monitor = recording_runtime::start_recording_shortcut(
        &windows.ui,
        overlays,
        core.shortcut_controller.clone(),
        ShortcutRuntime {
            recorder: Arc::clone(&core.recorder),
            microphone_access: core.microphone_access.clone(),
            first_recording: Arc::new(AtomicBool::new(true)),
            session: Arc::clone(&core.session),
            cancelled: Arc::clone(&core.cancelled),
            paused: Arc::clone(&paused),
            onboarding_toggle: {
                let shortcut = core.onboarding.shortcut_handler();
                Arc::new(move || shortcut.handle_toggle())
            },
            dictation: core.dictation.clone(),
            feedback_sounds_enabled: Arc::clone(&core.feedback_sounds_enabled),
            mute_system_audio_enabled: Arc::clone(&core.mute_system_audio_enabled),
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
    Ok((shortcut_monitor, tray))
}
