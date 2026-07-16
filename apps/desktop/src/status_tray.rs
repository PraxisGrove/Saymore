use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use slint::ComponentHandle;
use template_app::LocalSettingsStore;
use template_infra::{SqliteStorage, activate_application};

use crate::ui::{AppPage, AppWindow, SettingsSection, StatusTray};

pub fn wire(
    tray: &StatusTray,
    ui: &AppWindow,
    storage: Arc<SqliteStorage>,
    settings_guard: Arc<Mutex<()>>,
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
    let pause_storage = Arc::clone(&storage);
    let pause_guard = Arc::clone(&settings_guard);
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
            Arc::clone(&pause_storage),
            Arc::clone(&pause_guard),
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
    if let Err(error) = activate_application() {
        tracing::warn!(event = "tray.application_activate_failed", reason = %error);
    }
}

fn save_pause_setting(
    storage: Arc<SqliteStorage>,
    settings_guard: Arc<Mutex<()>>,
    paused: Arc<AtomicBool>,
    tray: slint::Weak<StatusTray>,
    enabled: bool,
) {
    let rollback_paused = Arc::clone(&paused);
    let rollback_tray = tray.clone();
    let spawn_result = thread::Builder::new()
        .name("saymore-save-pause-setting".to_owned())
        .spawn(move || {
            let result = settings_guard.lock().map_err(|_| ()).and_then(|_guard| {
                let mut settings = storage.load_settings().map_err(|_| ())?;
                settings.dictation_paused = enabled;
                storage.save_settings(settings).map_err(|_| ())
            });
            if result.is_err() {
                rollback_pause(paused, tray, enabled);
            }
        });
    if spawn_result.is_err() {
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
