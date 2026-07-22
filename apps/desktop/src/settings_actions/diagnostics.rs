use std::{
    io,
    path::{Path, PathBuf},
    thread,
};

use slint::{ComponentHandle, SharedString};
use template_app::LocalSettingsChange;

use crate::{
    diagnostics::{DiagnosticsController, DiagnosticsReportText},
    local_settings_runtime::LocalSettingsHandle,
    ui::{AppWindow, Translations},
};

pub(super) fn set_logging(
    ui: slint::Weak<AppWindow>,
    settings: LocalSettingsHandle,
    diagnostics: DiagnosticsController,
    enabled: bool,
) {
    let previous = diagnostics.is_enabled();
    if let Some(window) = ui.upgrade() {
        window.set_diagnostics_status(SharedString::new());
    }
    let failure_ui = ui.clone();
    let result = settings.submit(
        LocalSettingsChange::SetDiagnosticsLogging(enabled),
        move |result| {
            if let Some(window) = ui.upgrade() {
                match result {
                    Ok(_) => {
                        diagnostics.set_enabled(enabled);
                        window.set_diagnostics_enabled(enabled);
                        window.set_diagnostics_status(SharedString::new());
                    }
                    Err(error) => {
                        tracing::warn!(event = "settings.diagnostics_save_failed", reason = %error);
                        window.set_diagnostics_enabled(previous);
                        window.set_diagnostics_status(
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
        tracing::warn!(event = "settings.diagnostics_submit_failed", reason = %error);
        window.set_diagnostics_enabled(previous);
        window.set_diagnostics_status(window.global::<Translations>().get_settings_save_failed());
    }
}

pub(super) fn export_report(ui: slint::Weak<AppWindow>, diagnostics: DiagnosticsController) {
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
        tracing::info!(
            target: "saymore::diagnostics",
            event = "diagnostics.export_already_running"
        );
        return;
    }
    tracing::info!(
        target: "saymore::diagnostics",
        event = "diagnostics.export_started"
    );
    window.set_diagnostics_export_status(SharedString::from("exporting"));
    window.set_diagnostics_export_detail(
        window.global::<Translations>().get_diagnostics_generating(),
    );
    start_report_export(ui, diagnostics, destination, report_text(&window));
}

fn start_report_export(
    ui: slint::Weak<AppWindow>,
    diagnostics: DiagnosticsController,
    destination: PathBuf,
    report_text: DiagnosticsReportText,
) {
    let failure_ui = ui.clone();
    let worker_diagnostics = diagnostics.clone();
    let spawn = thread::Builder::new()
        .name("saymore-export-diagnostics".to_owned())
        .spawn(move || run_report_export(ui, worker_diagnostics, destination, report_text));
    if spawn.is_err() {
        diagnostics.finish_export();
        tracing::warn!(
            target: "saymore::diagnostics",
            event = "diagnostics.export_worker_start_failed"
        );
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

fn run_report_export(
    ui: slint::Weak<AppWindow>,
    diagnostics: DiagnosticsController,
    destination: PathBuf,
    report_text: DiagnosticsReportText,
) {
    let result = diagnostics.export_report(&destination, &report_text);
    diagnostics.finish_export();
    match &result {
        Ok(()) => tracing::info!(
            target: "saymore::diagnostics",
            event = "diagnostics.export_completed"
        ),
        Err(error) => tracing::warn!(
            target: "saymore::diagnostics",
            event = "diagnostics.export_failed",
            reason = %error
        ),
    }
    let _ =
        ui.upgrade_in_event_loop(move |window| apply_export_result(&window, &destination, result));
}

fn apply_export_result(window: &AppWindow, destination: &Path, result: Result<(), io::Error>) {
    match result {
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
    }
}

fn report_text(window: &AppWindow) -> DiagnosticsReportText {
    let translations = window.global::<Translations>();
    let generated = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
        .to_string();
    DiagnosticsReportText {
        title: translations.get_diagnostics_report_title().to_string(),
        version: translations
            .invoke_diagnostics_report_version(env!("CARGO_PKG_VERSION").into())
            .to_string(),
        generated: translations
            .invoke_diagnostics_report_generated(generated.into())
            .to_string(),
        privacy: translations.get_diagnostics_report_privacy().to_string(),
        events: translations.get_diagnostics_report_events().to_string(),
        no_events: translations.get_diagnostics_report_no_events().to_string(),
    }
}
