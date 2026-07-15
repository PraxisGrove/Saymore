use std::{
    path::PathBuf,
    process::Command,
    sync::{Arc, Mutex},
    thread,
};

use slint::{ComponentHandle, SharedString};
use template_app::LocalSettingsStore;
use template_infra::SqliteStorage;

use crate::{
    diagnostics::{DiagnosticsController, DiagnosticsReportText},
    ui::{AppWindow, Translations},
};

pub fn wire(
    ui: &AppWindow,
    storage: Arc<SqliteStorage>,
    settings_guard: Arc<Mutex<()>>,
    diagnostics: DiagnosticsController,
    data_directory: PathBuf,
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
