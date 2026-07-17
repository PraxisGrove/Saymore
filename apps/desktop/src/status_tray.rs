use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use slint::ComponentHandle;
#[cfg(target_os = "windows")]
use slint::winit_030::WinitWindowAccessor;
use template_app::LocalSettingsChange;
#[cfg(target_os = "macos")]
use template_infra::activate_application;

use crate::{
    local_settings_runtime::LocalSettingsHandle,
    ui::{AppPage, AppWindow, SettingsSection, StatusTray},
};

pub fn wire(
    tray: &StatusTray,
    ui: &AppWindow,
    settings: LocalSettingsHandle,
    paused: Arc<AtomicBool>,
    pause_recording: impl Fn() + 'static,
) {
    tray.set_paused(paused.load(Ordering::Acquire));

    let show_ui = ui.as_weak();
    tray.on_show_main_window(move || show_window(&show_ui, None));

    let settings_ui = ui.as_weak();
    tray.on_show_settings(move || {
        show_window(&settings_ui, Some(SettingsSection::General));
    });

    let pause_tray = tray.as_weak();
    let pause_settings = settings;
    tray.on_toggle_pause(move || {
        let enabled = !paused.load(Ordering::Acquire);
        paused.store(enabled, Ordering::Release);
        if let Some(tray) = pause_tray.upgrade() {
            tray.set_paused(enabled);
        }
        if enabled {
            pause_recording();
        }
        save_pause_setting(
            pause_settings.clone(),
            Arc::clone(&paused),
            pause_tray.clone(),
            enabled,
        );
    });

    tray.on_quit_application(|| {
        if let Err(error) = slint::quit_event_loop() {
            tracing::warn!(event = "tray.quit_failed", reason = %error);
        }
    });
}

pub(crate) fn show_window(ui: &slint::Weak<AppWindow>, section: Option<SettingsSection>) {
    let Some(window) = ui.upgrade() else {
        return;
    };
    if let Some(section) = section {
        window.set_current_page(AppPage::Settings);
        window.set_settings_section(section);
    }
    if let Err(error) = window.show() {
        tracing::warn!(event = "tray.window_show_failed", reason = %error);
        return;
    }
    if let Err(error) = activate_main_window(&window) {
        tracing::warn!(event = "tray.application_activate_failed", reason = %error);
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn activate_main_window(_window: &AppWindow) -> Result<(), String> {
    activate_application().map_err(|error| error.to_string())
}

#[cfg(target_os = "windows")]
pub(crate) fn activate_main_window(window: &AppWindow) -> Result<(), String> {
    window
        .window()
        .with_winit_window(|window| window.focus_window())
        .ok_or_else(|| "the Windows main window is not available".to_owned())
}

fn save_pause_setting(
    settings: LocalSettingsHandle,
    paused: Arc<AtomicBool>,
    tray: slint::Weak<StatusTray>,
    enabled: bool,
) {
    let rollback_paused = Arc::clone(&paused);
    let rollback_tray = tray.clone();
    let result = settings.submit(
        LocalSettingsChange::SetDictationPaused(enabled),
        move |result| {
            if let Err(error) = result {
                tracing::warn!(event = "tray.pause_save_failed", reason = %error);
                rollback_pause(paused, tray, enabled);
            }
        },
    );
    if let Err(error) = result {
        tracing::warn!(event = "tray.pause_submit_failed", reason = %error);
        rollback_pause(rollback_paused, rollback_tray, enabled);
    }
}

fn rollback_pause(paused: Arc<AtomicBool>, tray: slint::Weak<StatusTray>, attempted: bool) {
    paused.store(!attempted, Ordering::Release);
    let _ = tray.upgrade_in_event_loop(move |tray| {
        tray.set_paused(!attempted);
    });
    tracing::warn!(
        event = "tray.pause_save_failed",
        reason = "settings write failed"
    );
}
