use std::{
    path::PathBuf,
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use template_app::LocalSettingsStore;
use template_infra::SqliteStorage;
#[cfg(target_os = "macos")]
use template_infra::{
    LaunchAtLoginStatus, MacOsShortcut, MacOsShortcutController, MacOsShortcutError,
    dock_is_visible, launch_at_login_status, set_dock_visible, set_launch_at_login,
};

use crate::{
    diagnostics::{DiagnosticsController, DiagnosticsReportText},
    ui::{AppWindow, Translations},
};

mod shortcut;

pub fn wire(
    ui: &AppWindow,
    storage: Arc<SqliteStorage>,
    settings_guard: Arc<Mutex<()>>,
    feedback_sounds_enabled: Arc<AtomicBool>,
    diagnostics: DiagnosticsController,
    data_directory: PathBuf,
    shortcut_controller: MacOsShortcutController,
) {
    let logging_ui = ui.as_weak();
    let logging_storage = Arc::clone(&storage);
    let logging_guard = Arc::clone(&settings_guard);
    let logging_diagnostics = diagnostics.clone();
    ui.on_set_diagnostics_enabled(move |enabled| {
        set_diagnostics_logging(
            logging_ui.clone(),
            Arc::clone(&logging_storage),
            Arc::clone(&logging_guard),
            logging_diagnostics.clone(),
            enabled,
        );
    });

    let update_ui = ui.as_weak();
    let update_storage = Arc::clone(&storage);
    let update_guard = Arc::clone(&settings_guard);
    ui.on_set_automatic_update_checks(move |enabled| {
        set_automatic_update_checks(
            update_ui.clone(),
            Arc::clone(&update_storage),
            Arc::clone(&update_guard),
            enabled,
        );
    });

    let feedback_ui = ui.as_weak();
    let feedback_storage = Arc::clone(&storage);
    let feedback_guard = Arc::clone(&settings_guard);
    let feedback_state = Arc::clone(&feedback_sounds_enabled);
    ui.on_set_feedback_sounds_enabled(move |enabled| {
        set_feedback_sounds_enabled(
            feedback_ui.clone(),
            Arc::clone(&feedback_storage),
            Arc::clone(&feedback_guard),
            Arc::clone(&feedback_state),
            enabled,
        );
    });

    let clipboard_ui = ui.as_weak();
    let clipboard_storage = Arc::clone(&storage);
    let clipboard_guard = Arc::clone(&settings_guard);
    ui.on_set_copy_to_clipboard(move |enabled| {
        set_copy_to_clipboard(
            clipboard_ui.clone(),
            Arc::clone(&clipboard_storage),
            Arc::clone(&clipboard_guard),
            enabled,
        );
    });

    wire_platform_settings(ui, Arc::clone(&storage), Arc::clone(&settings_guard));
    shortcut::wire_shortcut_settings(
        ui,
        Arc::clone(&storage),
        Arc::clone(&settings_guard),
        shortcut_controller,
    );

    let export_ui = ui.as_weak();
    let export_diagnostics = diagnostics;
    ui.on_export_diagnostics_report(move || {
        export_report(export_ui.clone(), export_diagnostics.clone());
    });

    let data_ui = ui.as_weak();
    ui.on_open_data_directory(move || {
        open_data_directory(data_ui.clone(), data_directory.clone());
    });

    if let Ok(settings) = storage.load_settings() {
        ui.set_diagnostics_enabled(settings.diagnostics_logging_enabled);
        ui.set_automatic_update_checks(settings.automatic_update_checks);
        ui.set_feedback_sounds_enabled(settings.feedback_sounds_enabled);
        ui.set_copy_to_clipboard(settings.copy_to_clipboard);
        feedback_sounds_enabled.store(settings.feedback_sounds_enabled, Ordering::Release);
    }
}

#[cfg(target_os = "macos")]
fn wire_platform_settings(
    ui: &AppWindow,
    storage: Arc<SqliteStorage>,
    settings_guard: Arc<Mutex<()>>,
) {
    let dock_ui = ui.as_weak();
    let dock_storage = Arc::clone(&storage);
    let dock_guard = Arc::clone(&settings_guard);
    ui.on_set_show_in_dock(move |visible| {
        set_show_in_dock(
            dock_ui.clone(),
            Arc::clone(&dock_storage),
            Arc::clone(&dock_guard),
            visible,
        );
    });

    let login_ui = ui.as_weak();
    ui.on_set_launch_at_login(move |enabled| {
        let Some(window) = login_ui.upgrade() else {
            return;
        };
        let previous = window.get_launch_at_login();
        window.set_launch_at_login_status(SharedString::new());
        match set_launch_at_login(enabled).and_then(|()| launch_at_login_status().map(|_| ())) {
            Ok(()) => window.set_launch_at_login(enabled),
            Err(error) => {
                tracing::warn!(event = "settings.launch_at_login_failed", reason = %error);
                window.set_launch_at_login(previous);
                window.set_launch_at_login_status(
                    window.global::<Translations>().get_settings_save_failed(),
                );
            }
        }
    });

    if let Ok(settings) = storage.load_settings() {
        let visible = set_dock_visible(settings.show_in_dock)
            .and_then(|()| dock_is_visible())
            .unwrap_or(true);
        ui.set_show_in_dock(visible);
    }
    match launch_at_login_status() {
        Ok(LaunchAtLoginStatus::Enabled | LaunchAtLoginStatus::RequiresApproval) => {
            ui.set_launch_at_login(true);
        }
        Ok(LaunchAtLoginStatus::Disabled) => ui.set_launch_at_login(false),
        Err(error) => {
            tracing::warn!(event = "settings.launch_at_login_status_failed", reason = %error);
            ui.set_launch_at_login(false);
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn wire_platform_settings(
    _ui: &AppWindow,
    _storage: Arc<SqliteStorage>,
    _settings_guard: Arc<Mutex<()>>,
) {
}

#[cfg(target_os = "macos")]
fn set_show_in_dock(
    ui: slint::Weak<AppWindow>,
    storage: Arc<SqliteStorage>,
    settings_guard: Arc<Mutex<()>>,
    visible: bool,
) {
    let Some(window) = ui.upgrade() else {
        return;
    };
    let previous = window.get_show_in_dock();
    window.set_show_in_dock_status(SharedString::new());
    if let Err(error) = set_dock_visible(visible) {
        tracing::warn!(event = "settings.dock_visibility_failed", reason = %error);
        window.set_show_in_dock(previous);
        window.set_show_in_dock_status(window.global::<Translations>().get_settings_save_failed());
        return;
    }

    let failure_ui = ui.clone();
    if thread::Builder::new()
        .name("saymore-save-dock-setting".to_owned())
        .spawn(move || {
            let result = settings_guard.lock().map_err(|_| ()).and_then(|_guard| {
                let mut settings = storage.load_settings().map_err(|_| ())?;
                settings.show_in_dock = visible;
                storage.save_settings(settings).map_err(|_| ())
            });
            let _ = ui.upgrade_in_event_loop(move |window| match result {
                Ok(()) => window.set_show_in_dock(visible),
                Err(()) => {
                    let _ = set_dock_visible(previous);
                    window.set_show_in_dock(previous);
                    window.set_show_in_dock_status(
                        window.global::<Translations>().get_settings_save_failed(),
                    );
                }
            });
        })
        .is_err()
        && let Some(window) = failure_ui.upgrade()
    {
        let _ = set_dock_visible(previous);
        window.set_show_in_dock(previous);
        window.set_show_in_dock_status(window.global::<Translations>().get_settings_save_failed());
    }
}

fn set_copy_to_clipboard(
    ui: slint::Weak<AppWindow>,
    storage: Arc<SqliteStorage>,
    settings_guard: Arc<Mutex<()>>,
    enabled: bool,
) {
    let Some(window) = ui.upgrade() else {
        return;
    };
    let previous = window.get_copy_to_clipboard();
    window.set_copy_to_clipboard_status(SharedString::new());
    let failure_ui = ui.clone();
    if thread::Builder::new()
        .name("saymore-save-clipboard-setting".to_owned())
        .spawn(move || {
            let result = settings_guard.lock().map_err(|_| ()).and_then(|_guard| {
                let mut settings = storage.load_settings().map_err(|_| ())?;
                settings.copy_to_clipboard = enabled;
                storage.save_settings(settings).map_err(|_| ())
            });
            let _ = ui.upgrade_in_event_loop(move |window| match result {
                Ok(()) => {
                    window.set_copy_to_clipboard(enabled);
                    window.set_copy_to_clipboard_status(SharedString::new());
                }
                Err(()) => {
                    window.set_copy_to_clipboard(previous);
                    window.set_copy_to_clipboard_status(
                        window.global::<Translations>().get_settings_save_failed(),
                    );
                }
            });
        })
        .is_err()
        && let Some(window) = failure_ui.upgrade()
    {
        window.set_copy_to_clipboard(previous);
        window.set_copy_to_clipboard_status(
            window.global::<Translations>().get_settings_save_failed(),
        );
    }
}

fn set_automatic_update_checks(
    ui: slint::Weak<AppWindow>,
    storage: Arc<SqliteStorage>,
    settings_guard: Arc<Mutex<()>>,
    enabled: bool,
) {
    let Some(window) = ui.upgrade() else {
        return;
    };
    let previous = window.get_automatic_update_checks();
    window.set_automatic_update_status(SharedString::new());
    let failure_ui = ui.clone();
    if thread::Builder::new()
        .name("saymore-save-update-check-setting".to_owned())
        .spawn(move || {
            let result = settings_guard.lock().map_err(|_| ()).and_then(|_guard| {
                let mut settings = storage.load_settings().map_err(|_| ())?;
                settings.automatic_update_checks = enabled;
                storage.save_settings(settings).map_err(|_| ())
            });
            let _ = ui.upgrade_in_event_loop(move |window| match result {
                Ok(()) => {
                    window.set_automatic_update_checks(enabled);
                    window.set_automatic_update_status(SharedString::new());
                    if enabled {
                        window.invoke_check_for_updates();
                    }
                }
                Err(()) => {
                    window.set_automatic_update_checks(previous);
                    window.set_automatic_update_status(
                        window.global::<Translations>().get_settings_save_failed(),
                    );
                }
            });
        })
        .is_err()
        && let Some(window) = failure_ui.upgrade()
    {
        window.set_automatic_update_checks(previous);
        window.set_automatic_update_status(
            window.global::<Translations>().get_settings_save_failed(),
        );
    }
}

fn set_feedback_sounds_enabled(
    ui: slint::Weak<AppWindow>,
    storage: Arc<SqliteStorage>,
    settings_guard: Arc<Mutex<()>>,
    feedback_sounds_enabled: Arc<AtomicBool>,
    enabled: bool,
) {
    let Some(window) = ui.upgrade() else {
        return;
    };
    let previous = feedback_sounds_enabled.load(Ordering::Acquire);
    window.set_feedback_sounds_status(SharedString::new());
    let failure_ui = ui.clone();
    if thread::Builder::new()
        .name("saymore-save-feedback-sound-setting".to_owned())
        .spawn(move || {
            let result = settings_guard.lock().map_err(|_| ()).and_then(|_guard| {
                let mut settings = storage.load_settings().map_err(|_| ())?;
                settings.feedback_sounds_enabled = enabled;
                storage.save_settings(settings).map_err(|_| ())
            });
            let _ = ui.upgrade_in_event_loop(move |window| match result {
                Ok(()) => {
                    feedback_sounds_enabled.store(enabled, Ordering::Release);
                    window.set_feedback_sounds_enabled(enabled);
                    window.set_feedback_sounds_status(SharedString::new());
                }
                Err(()) => {
                    feedback_sounds_enabled.store(previous, Ordering::Release);
                    window.set_feedback_sounds_enabled(previous);
                    window.set_feedback_sounds_status(
                        window.global::<Translations>().get_settings_save_failed(),
                    );
                }
            });
        })
        .is_err()
        && let Some(window) = failure_ui.upgrade()
    {
        window.set_feedback_sounds_enabled(previous);
        window
            .set_feedback_sounds_status(window.global::<Translations>().get_settings_save_failed());
    }
}

fn set_diagnostics_logging(
    ui: slint::Weak<AppWindow>,
    storage: Arc<SqliteStorage>,
    settings_guard: Arc<Mutex<()>>,
    diagnostics: DiagnosticsController,
    enabled: bool,
) {
    let previous = diagnostics.is_enabled();
    if let Some(window) = ui.upgrade() {
        window.set_diagnostics_status(SharedString::new());
    }

    let failure_ui = ui.clone();
    if thread::Builder::new()
        .name("saymore-save-diagnostics-setting".to_owned())
        .spawn(move || {
            let result = settings_guard.lock().map_err(|_| ()).and_then(|_guard| {
                let mut settings = storage.load_settings().map_err(|_| ())?;
                settings.diagnostics_logging_enabled = enabled;
                storage.save_settings(settings).map_err(|_| ())
            });
            let _ = ui.upgrade_in_event_loop(move |window| match result {
                Ok(()) => {
                    diagnostics.set_enabled(enabled);
                    window.set_diagnostics_enabled(enabled);
                    window.set_diagnostics_status(SharedString::new());
                }
                Err(()) => {
                    window.set_diagnostics_enabled(previous);
                    window.set_diagnostics_status(
                        window.global::<Translations>().get_settings_save_failed(),
                    );
                }
            });
        })
        .is_err()
        && let Some(window) = failure_ui.upgrade()
    {
        window.set_diagnostics_enabled(previous);
        window.set_diagnostics_status(window.global::<Translations>().get_settings_save_failed());
    }
}

fn export_report(ui: slint::Weak<AppWindow>, diagnostics: DiagnosticsController) {
    let Some(window) = ui.upgrade() else {
        return;
    };
    let destination = rfd::FileDialog::new()
        .set_file_name(
            window
                .global::<Translations>()
                .get_diagnostics_report_file_name()
                .as_str(),
        )
        .save_file();
    let Some(destination) = destination else {
        return;
    };
    if !diagnostics.begin_export() {
        return;
    }
    let translations = window.global::<Translations>();
    let report_text = DiagnosticsReportText {
        title: translations.get_diagnostics_report_title().to_string(),
        version: translations
            .invoke_diagnostics_report_version(env!("CARGO_PKG_VERSION").into())
            .to_string(),
        generated: translations
            .invoke_diagnostics_report_generated(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |duration| duration.as_secs())
                    .to_string()
                    .into(),
            )
            .to_string(),
        privacy: translations.get_diagnostics_report_privacy().to_string(),
        events: translations.get_diagnostics_report_events().to_string(),
        no_events: translations.get_diagnostics_report_no_events().to_string(),
    };
    if let Some(window) = ui.upgrade() {
        window.set_diagnostics_export_status(SharedString::from("exporting"));
        window.set_diagnostics_export_detail(
            window.global::<Translations>().get_diagnostics_generating(),
        );
    }

    let failure_ui = ui.clone();
    let worker_diagnostics = diagnostics.clone();
    if thread::Builder::new()
        .name("saymore-export-diagnostics".to_owned())
        .spawn(move || {
            let result = worker_diagnostics.export_report(&destination, &report_text);
            worker_diagnostics.finish_export();
            let _ = ui.upgrade_in_event_loop(move |window| match result {
                Ok(()) => {
                    let file_name = destination
                        .file_name()
                        .and_then(|name| name.to_str())
                        .map(ToOwned::to_owned)
                        .unwrap_or_else(|| {
                            window
                                .global::<Translations>()
                                .get_diagnostics_report_name()
                                .to_string()
                        });
                    window.set_diagnostics_export_status(SharedString::from("success"));
                    window.set_diagnostics_export_detail(
                        window
                            .global::<Translations>()
                            .invoke_diagnostics_exported(file_name.into()),
                    );
                }
                Err(_) => {
                    window.set_diagnostics_export_status(SharedString::from("failed"));
                    window.set_diagnostics_export_detail(
                        window
                            .global::<Translations>()
                            .get_diagnostics_export_failed(),
                    );
                }
            });
        })
        .is_err()
    {
        diagnostics.finish_export();
        if let Some(window) = failure_ui.upgrade() {
            window.set_diagnostics_export_status(SharedString::from("failed"));
            window.set_diagnostics_export_detail(
                window
                    .global::<Translations>()
                    .get_diagnostics_export_start_failed(),
            );
        }
    }
}

fn open_data_directory(ui: slint::Weak<AppWindow>, directory: PathBuf) {
    if Command::new("/usr/bin/open")
        .arg(directory)
        .spawn()
        .is_err()
        && let Some(window) = ui.upgrade()
    {
        window.set_diagnostics_status(
            window
                .global::<Translations>()
                .get_diagnostics_open_data_folder_failed(),
        );
    }
}
