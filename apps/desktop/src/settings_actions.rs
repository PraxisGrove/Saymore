use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use slint::{ComponentHandle, SharedString};
use template_app::{LocalSettingsChange, LocalSettingsStore};
use template_infra::AppEnvironment;
use template_infra::SqliteStorage;
#[cfg(target_os = "windows")]
use template_infra::WindowsLaunchAtLogin;
#[cfg(target_os = "macos")]
use template_infra::{
    LaunchAtLoginStatus, MacOsShortcut, MacOsShortcutController, MacOsShortcutError,
    dock_is_visible, launch_at_login_status, set_dock_visible, set_launch_at_login,
};

#[cfg(target_os = "macos")]
pub(crate) type PlatformShortcutController = MacOsShortcutController;

#[cfg(target_os = "windows")]
pub(crate) type PlatformShortcutController = template_infra::WindowsShortcutController;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[derive(Clone, Copy, Default)]
pub(crate) struct PlatformShortcutController;

pub(crate) struct PlatformOptions {
    pub(crate) data_directory: PathBuf,
    pub(crate) shortcut_controller: PlatformShortcutController,
    pub(crate) environment: AppEnvironment,
}

use crate::{
    diagnostics::DiagnosticsController,
    local_settings_runtime::LocalSettingsHandle,
    ui::{AppWindow, Translations},
};

#[cfg(target_os = "macos")]
mod shortcut;

mod audio;
use audio::{set_feedback_sounds_enabled, set_mute_system_audio_enabled};
mod diagnostics;

#[cfg(target_os = "windows")]
#[path = "settings_actions/windows_shortcut.rs"]
mod shortcut;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod shortcut {
    use super::*;

    pub(super) fn wire_shortcut_settings(
        ui: &AppWindow,
        _settings: LocalSettingsHandle,
        controller: PlatformShortcutController,
    ) {
        let status = ui.global::<Translations>().get_shortcut_unsupported();
        let _ = controller;
        ui.set_shortcut_status(SharedString::new());
        let add_ui = ui.as_weak();
        let add_status = status.clone();
        ui.on_begin_shortcut_capture(move || {
            if let Some(ui) = add_ui.upgrade() {
                ui.set_shortcut_status(add_status.clone());
            }
        });
        let edit_ui = ui.as_weak();
        let edit_status = status.clone();
        ui.on_edit_shortcut(move |_| {
            if let Some(ui) = edit_ui.upgrade() {
                ui.set_shortcut_status(edit_status.clone());
            }
        });
        let remove_ui = ui.as_weak();
        ui.on_remove_shortcut(move |_| {
            if let Some(ui) = remove_ui.upgrade() {
                ui.set_shortcut_status(status.clone());
            }
        });
    }
}

pub fn wire(
    ui: &AppWindow,
    storage: Arc<SqliteStorage>,
    settings: LocalSettingsHandle,
    feedback_sounds_enabled: Arc<AtomicBool>,
    mute_system_audio_enabled: Arc<AtomicBool>,
    diagnostics: DiagnosticsController,
    options: PlatformOptions,
) {
    let logging_ui = ui.as_weak();
    let logging_settings = settings.clone();
    let logging_diagnostics = diagnostics.clone();
    ui.on_set_diagnostics_enabled(move |enabled| {
        diagnostics::set_logging(
            logging_ui.clone(),
            logging_settings.clone(),
            logging_diagnostics.clone(),
            enabled,
        );
    });

    let update_ui = ui.as_weak();
    let update_settings = settings.clone();
    ui.on_set_automatic_update_checks(move |enabled| {
        set_automatic_update_checks(update_ui.clone(), update_settings.clone(), enabled);
    });

    let feedback_ui = ui.as_weak();
    let feedback_settings = settings.clone();
    let feedback_state = Arc::clone(&feedback_sounds_enabled);
    ui.on_set_feedback_sounds_enabled(move |enabled| {
        set_feedback_sounds_enabled(
            feedback_ui.clone(),
            feedback_settings.clone(),
            Arc::clone(&feedback_state),
            enabled,
        );
    });

    let mute_ui = ui.as_weak();
    let mute_settings = settings.clone();
    let mute_state = Arc::clone(&mute_system_audio_enabled);
    ui.on_set_mute_system_audio_enabled(move |enabled| {
        set_mute_system_audio_enabled(
            mute_ui.clone(),
            mute_settings.clone(),
            Arc::clone(&mute_state),
            enabled,
        );
    });

    let clipboard_ui = ui.as_weak();
    let clipboard_settings = settings.clone();
    ui.on_set_copy_to_clipboard(move |enabled| {
        set_copy_to_clipboard(clipboard_ui.clone(), clipboard_settings.clone(), enabled);
    });

    wire_platform_settings(
        ui,
        Arc::clone(&storage),
        settings.clone(),
        options.environment,
    );
    shortcut::wire_shortcut_settings(ui, settings, options.shortcut_controller);

    let export_ui = ui.as_weak();
    let export_diagnostics = diagnostics;
    ui.on_export_diagnostics_report(move || {
        diagnostics::export_report(export_ui.clone(), export_diagnostics.clone());
    });

    let data_ui = ui.as_weak();
    ui.on_open_data_directory(move || {
        open_data_directory(data_ui.clone(), options.data_directory.clone());
    });

    if let Ok(settings) = storage.load_settings() {
        ui.set_diagnostics_enabled(settings.diagnostics_logging_enabled);
        ui.set_automatic_update_checks(settings.automatic_update_checks);
        ui.set_feedback_sounds_enabled(settings.feedback_sounds_enabled);
        ui.set_mute_system_audio_enabled(settings.mute_system_audio_enabled);
        ui.set_copy_to_clipboard(settings.copy_to_clipboard);
        feedback_sounds_enabled.store(settings.feedback_sounds_enabled, Ordering::Release);
        mute_system_audio_enabled.store(settings.mute_system_audio_enabled, Ordering::Release);
    }
}

#[cfg(target_os = "macos")]
fn wire_platform_settings(
    ui: &AppWindow,
    storage: Arc<SqliteStorage>,
    settings: LocalSettingsHandle,
    _environment: AppEnvironment,
) {
    let dock_ui = ui.as_weak();
    let dock_settings = settings;
    let dock_pending = Arc::new(AtomicBool::new(false));
    ui.on_set_show_in_dock(move |visible| {
        set_show_in_dock(
            dock_ui.clone(),
            dock_settings.clone(),
            Arc::clone(&dock_pending),
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

#[cfg(target_os = "windows")]
fn wire_platform_settings(
    ui: &AppWindow,
    _storage: Arc<SqliteStorage>,
    _settings: LocalSettingsHandle,
    environment: AppEnvironment,
) {
    let integration = match WindowsLaunchAtLogin::for_current_executable(environment) {
        Ok(integration) => Arc::new(integration),
        Err(error) => {
            tracing::warn!(event = "settings.launch_at_login_init_failed", reason = %error);
            ui.set_launch_at_login(false);
            ui.set_launch_at_login_status(ui.global::<Translations>().get_settings_save_failed());
            return;
        }
    };
    match integration.is_enabled() {
        Ok(enabled) => ui.set_launch_at_login(enabled),
        Err(error) => {
            tracing::warn!(event = "settings.launch_at_login_status_failed", reason = %error);
            ui.set_launch_at_login(false);
        }
    }
    let login_ui = ui.as_weak();
    ui.on_set_launch_at_login(move |enabled| {
        let Some(window) = login_ui.upgrade() else {
            return;
        };
        let previous = window.get_launch_at_login();
        window.set_launch_at_login_status(SharedString::new());
        match integration.set_enabled(enabled) {
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
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn wire_platform_settings(
    _ui: &AppWindow,
    _storage: Arc<SqliteStorage>,
    _settings: LocalSettingsHandle,
    _environment: AppEnvironment,
) {
}

#[cfg(target_os = "macos")]
fn set_show_in_dock(
    ui: slint::Weak<AppWindow>,
    settings: LocalSettingsHandle,
    pending: Arc<AtomicBool>,
    visible: bool,
) {
    let Some(window) = ui.upgrade() else {
        return;
    };
    let previous = window.get_show_in_dock();
    if pending.swap(true, Ordering::AcqRel) {
        window.set_show_in_dock(dock_is_visible().unwrap_or(previous));
        return;
    }
    window.set_show_in_dock_status(SharedString::new());
    if let Err(error) = set_dock_visible(visible) {
        pending.store(false, Ordering::Release);
        tracing::warn!(event = "settings.dock_visibility_failed", reason = %error);
        window.set_show_in_dock(previous);
        window.set_show_in_dock_status(window.global::<Translations>().get_settings_save_failed());
        return;
    }

    let failure_ui = ui.clone();
    let completion_pending = Arc::clone(&pending);
    let result = settings.submit(
        LocalSettingsChange::SetDockVisibility(visible),
        move |result| {
            completion_pending.store(false, Ordering::Release);
            let Some(window) = ui.upgrade() else {
                return;
            };
            match result {
                Ok(_) => window.set_show_in_dock(visible),
                Err(error) => {
                    tracing::warn!(event = "settings.dock_save_failed", reason = %error);
                    let _ = set_dock_visible(previous);
                    window.set_show_in_dock(previous);
                    window.set_show_in_dock_status(
                        window.global::<Translations>().get_settings_save_failed(),
                    );
                }
            }
        },
    );
    if result.is_err() {
        pending.store(false, Ordering::Release);
    }
    if let Err(error) = result
        && let Some(window) = failure_ui.upgrade()
    {
        tracing::warn!(event = "settings.dock_submit_failed", reason = %error);
        let _ = set_dock_visible(previous);
        window.set_show_in_dock(previous);
        window.set_show_in_dock_status(window.global::<Translations>().get_settings_save_failed());
    }
}

fn set_copy_to_clipboard(ui: slint::Weak<AppWindow>, settings: LocalSettingsHandle, enabled: bool) {
    let Some(window) = ui.upgrade() else {
        return;
    };
    let previous = window.get_copy_to_clipboard();
    window.set_copy_to_clipboard_status(SharedString::new());
    let failure_ui = ui.clone();
    let result = settings.submit(
        LocalSettingsChange::SetCopyToClipboard(enabled),
        move |result| {
            if let Some(window) = ui.upgrade() {
                match result {
                    Ok(_) => {
                        window.set_copy_to_clipboard(enabled);
                        window.set_copy_to_clipboard_status(SharedString::new());
                    }
                    Err(error) => {
                        tracing::warn!(event = "settings.clipboard_save_failed", reason = %error);
                        window.set_copy_to_clipboard(previous);
                        window.set_copy_to_clipboard_status(
                            window.global::<Translations>().get_settings_save_failed(),
                        );
                    }
                }
            }
        },
    );
    if let Err(error) = result
        && let Some(window) = failure_ui.upgrade()
    {
        tracing::warn!(event = "settings.clipboard_submit_failed", reason = %error);
        window.set_copy_to_clipboard(previous);
        window.set_copy_to_clipboard_status(
            window.global::<Translations>().get_settings_save_failed(),
        );
    }
}

fn set_automatic_update_checks(
    ui: slint::Weak<AppWindow>,
    settings: LocalSettingsHandle,
    enabled: bool,
) {
    let Some(window) = ui.upgrade() else {
        return;
    };
    let previous = window.get_automatic_update_checks();
    window.set_automatic_update_status(SharedString::new());
    let failure_ui = ui.clone();
    let result = settings.submit(
        LocalSettingsChange::SetAutomaticUpdateChecks(enabled),
        move |result| {
            if let Some(window) = ui.upgrade() {
                match result {
                Ok(_) => {
                    window.set_automatic_update_checks(enabled);
                    window.set_automatic_update_status(SharedString::new());
                    if enabled {
                        window.invoke_check_for_updates();
                    }
                }
                Err(error) => {
                    tracing::warn!(event = "settings.update_check_save_failed", reason = %error);
                    window.set_automatic_update_checks(previous);
                    window.set_automatic_update_status(
                        window.global::<Translations>().get_settings_save_failed(),
                    );
                }
                }
            }
        },
    );
    if let Err(error) = result
        && let Some(window) = failure_ui.upgrade()
    {
        tracing::warn!(event = "settings.update_check_submit_failed", reason = %error);
        window.set_automatic_update_checks(previous);
        window.set_automatic_update_status(
            window.global::<Translations>().get_settings_save_failed(),
        );
    }
}

fn open_data_directory(ui: slint::Weak<AppWindow>, directory: PathBuf) {
    if crate::platform_open::open(directory).is_err()
        && let Some(window) = ui.upgrade()
    {
        window.set_diagnostics_status(
            window
                .global::<Translations>()
                .get_diagnostics_open_data_folder_failed(),
        );
    }
}
