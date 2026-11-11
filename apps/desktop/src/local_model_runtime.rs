use std::{
    io,
    sync::Arc,
    time::{Duration, Instant},
};

use slint::{ComponentHandle, SharedString};
use template_infra::{
    JsonSettingsStore, ModelInstallError, SqliteStorage, VerifiedModelInstaller,
    current_process_resident_memory_bytes,
};

use crate::{
    asr_runtime::AsrSessionController,
    settings_ui,
    ui::{AppWindow, AsrProviderCardKind, LocalModelDownloadState},
};

mod download;
mod import;
mod lifecycle;
mod model;
mod recognition_test;
mod retired;
mod state;
mod support;

use download::wire_download;
use lifecycle::{activate, clear, delete, prepare};
use model::LocalModel;
use recognition_test::wire as wire_test;
use state::*;
use support::*;

const MINIMUM_ACTIVATION_FEEDBACK: Duration = Duration::from_millis(650);

struct LocalModelInstallers {
    whisper: Arc<VerifiedModelInstaller>,
    qwen3: Arc<VerifiedModelInstaller>,
    sense_voice: Arc<VerifiedModelInstaller>,
    seaco: Arc<VerifiedModelInstaller>,
    zipformer: Arc<VerifiedModelInstaller>,
    moonshine: Arc<VerifiedModelInstaller>,
}

impl LocalModelInstallers {
    fn new(models_directory: &std::path::Path) -> Result<Self, io::Error> {
        Ok(Self {
            whisper: Arc::new(
                VerifiedModelInstaller::whisper_large_v3_turbo(models_directory.to_path_buf())
                    .map_err(runtime_error)?,
            ),
            qwen3: Arc::new(
                VerifiedModelInstaller::qwen3_asr_1_7b(models_directory.to_path_buf())
                    .map_err(runtime_error)?,
            ),
            sense_voice: Arc::new(
                VerifiedModelInstaller::sense_voice_small(models_directory.to_path_buf())
                    .map_err(runtime_error)?,
            ),
            seaco: Arc::new(
                VerifiedModelInstaller::seaco_paraformer(models_directory.to_path_buf())
                    .map_err(runtime_error)?,
            ),
            zipformer: Arc::new(
                VerifiedModelInstaller::zipformer(models_directory.to_path_buf())
                    .map_err(runtime_error)?,
            ),
            moonshine: Arc::new(
                VerifiedModelInstaller::moonshine(models_directory.to_path_buf())
                    .map_err(runtime_error)?,
            ),
        })
    }

    fn for_model(&self, model: LocalModel) -> Arc<VerifiedModelInstaller> {
        match model {
            LocalModel::Whisper => Arc::clone(&self.whisper),
            LocalModel::Qwen3 => Arc::clone(&self.qwen3),
            LocalModel::SenseVoice => Arc::clone(&self.sense_voice),
            LocalModel::Seaco => Arc::clone(&self.seaco),
            LocalModel::Zipformer => Arc::clone(&self.zipformer),
            LocalModel::Moonshine => Arc::clone(&self.moonshine),
        }
    }
}

pub(crate) fn wire(
    ui: &AppWindow,
    settings: Arc<JsonSettingsStore>,
    storage: Arc<SqliteStorage>,
    models_directory: std::path::PathBuf,
    asr: Arc<AsrSessionController>,
) -> Result<(), io::Error> {
    let device = template_infra::local_inference_device();
    tracing::info!(
        target: "saymore::diagnostics",
        event = "local_asr.device_capabilities",
        architecture = device.architecture,
        total_physical_memory_bytes = ?device.total_physical_memory_bytes,
        available_physical_memory_bytes = ?device.available_physical_memory_bytes,
        avx = device.avx,
        avx2 = device.avx2,
        cpu_backend_supported = device.cpu_backend_supported
    );
    let installers = Arc::new(LocalModelInstallers::new(&models_directory)?);
    for model in LocalModel::ALL {
        let installer = installers.for_model(model);
        apply_state(ui, &settings, &storage, model, &installer)?;
    }
    wire_download(ui, Arc::clone(&storage), Arc::clone(&installers));
    import::wire(
        ui,
        Arc::clone(&settings),
        Arc::clone(&storage),
        Arc::clone(&installers),
    );
    wire_selection(
        ui,
        Arc::clone(&settings),
        Arc::clone(&installers),
        Arc::clone(&asr),
    );
    wire_provider_selection_lifecycle(ui, Arc::clone(&asr));
    wire_runtime_memory_refresh(ui);
    wire_delete(
        ui,
        Arc::clone(&settings),
        Arc::clone(&storage),
        Arc::clone(&installers),
        Arc::clone(&asr),
        models_directory.clone(),
    );
    wire_test(ui, installers, Arc::clone(&asr));
    retired::wire(ui, Arc::clone(&storage), models_directory)?;
    for model in LocalModel::ALL {
        if model_selected(ui, model) {
            preload_selected_model(ui, model, Arc::clone(&asr));
        }
    }
    Ok(())
}

fn wire_runtime_memory_refresh(ui: &AppWindow) {
    let weak = ui.as_weak();
    ui.on_refresh_local_model_runtime_memory(move |kind| {
        let Some(model) = LocalModel::from_card(kind) else {
            return;
        };
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if model_selected(&ui, model)
            && let Some(memory_bytes) = current_process_resident_memory_bytes()
        {
            apply_runtime_memory(&ui, model, Some(memory_bytes));
        }
    });
}

fn wire_provider_selection_lifecycle(ui: &AppWindow, asr: Arc<AsrSessionController>) {
    let weak = ui.as_weak();
    ui.on_asr_provider_selection_applied(move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        for model in LocalModel::ALL
            .into_iter()
            .filter(|model| !model_selected(&ui, *model))
        {
            clear(asr.as_ref(), model);
            clear_runtime_memory(&ui, model);
        }
    });
}

fn remaining_activation_feedback(elapsed: Duration) -> Duration {
    MINIMUM_ACTIVATION_FEEDBACK.saturating_sub(elapsed)
}

fn installer_for(
    kind: AsrProviderCardKind,
    installers: &LocalModelInstallers,
) -> Option<(LocalModel, Arc<VerifiedModelInstaller>)> {
    let model = LocalModel::from_card(kind)?;
    Some((model, installers.for_model(model)))
}

fn wire_selection(
    ui: &AppWindow,
    settings: Arc<JsonSettingsStore>,
    installers: Arc<LocalModelInstallers>,
    asr: Arc<AsrSessionController>,
) {
    let weak = ui.as_weak();
    ui.on_request_local_model_selection(move |kind| {
        let Some((model, installer)) = installer_for(kind, &installers) else {
            return;
        };
        if !installer.is_installed() {
            return;
        }
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if activation_pending(&ui, model) || model_selected(&ui, model) {
            return;
        }
        let feedback_started = Instant::now();
        set_activation_pending(&ui, model, true);
        ui.set_local_model_operation_error(SharedString::default());
        let result_ui = weak.clone();
        let settings = Arc::clone(&settings);
        let asr = Arc::clone(&asr);
        let spawn = std::thread::Builder::new()
            .name(model.thread_name("activate"))
            .spawn(move || {
                let result = activate(settings.as_ref(), asr.as_ref(), model)
                    .map_err(|error| error.to_string())
                    .map(|activation| activation.memory_bytes);
                std::thread::sleep(remaining_activation_feedback(feedback_started.elapsed()));
                let event_settings = Arc::clone(&settings);
                let _ = result_ui.upgrade_in_event_loop(move |ui| match result {
                    Ok(memory_bytes) => {
                        settings_ui::reload_settings(&ui, &event_settings);
                        apply_runtime_memory(
                            &ui,
                            model,
                            current_process_resident_memory_bytes().or(memory_bytes),
                        );
                        set_activation_pending(&ui, model, false);
                    }
                    Err(error) => apply_activation_failure(&ui, model, error),
                });
            });
        if let Err(error) = spawn {
            apply_activation_failure(&ui, model, error.to_string());
        }
    });
}

fn preload_selected_model(ui: &AppWindow, model: LocalModel, asr: Arc<AsrSessionController>) {
    set_activation_pending(ui, model, true);
    ui.set_local_model_operation_error(SharedString::default());
    let result_ui = ui.as_weak();
    let spawn = std::thread::Builder::new()
        .name(model.thread_name("preload"))
        .spawn(move || {
            let result = prepare(asr.as_ref(), model).map_err(|error| error.to_string());
            let _ = result_ui.upgrade_in_event_loop(move |ui| {
                set_activation_pending(&ui, model, false);
                match result {
                    Ok(memory_bytes) => apply_runtime_memory(
                        &ui,
                        model,
                        current_process_resident_memory_bytes().or(memory_bytes),
                    ),
                    Err(error) => apply_activation_failure(&ui, model, error),
                }
            });
        });
    if let Err(error) = spawn {
        apply_activation_failure(ui, model, error.to_string());
    }
}

fn wire_delete(
    ui: &AppWindow,
    settings: Arc<JsonSettingsStore>,
    storage: Arc<SqliteStorage>,
    installers: Arc<LocalModelInstallers>,
    asr: Arc<AsrSessionController>,
    models_directory: std::path::PathBuf,
) {
    let weak = ui.as_weak();
    ui.on_request_local_model_delete(move |kind| {
        if kind == AsrProviderCardKind::Paraformer {
            if let Some(ui) = weak.upgrade() {
                retired::remove(
                    &ui,
                    &storage,
                    &models_directory,
                    template_infra::RetiredAsrModel::Paraformer,
                );
            }
            return;
        }
        let Some((model, installer)) = installer_for(kind, &installers) else {
            return;
        };
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if activation_pending(&ui, model) {
            return;
        }
        let result = delete(
            settings.as_ref(),
            storage.as_ref(),
            asr.as_ref(),
            installer.as_ref(),
            model,
        )
        .map_err(|error| error.to_string());
        match result {
            Ok(()) => {
                reset_model_ui(&ui, model);
                ui.invoke_refresh_usage();
            }
            Err(error) => apply_operation_failure(&ui, error),
        }
    });
}

fn runtime_error(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fast_model_switch_keeps_feedback_visible_for_the_minimum_duration() {
        assert_eq!(
            std::time::Duration::from_millis(500),
            remaining_activation_feedback(std::time::Duration::from_millis(150))
        );
        assert_eq!(
            std::time::Duration::ZERO,
            remaining_activation_feedback(std::time::Duration::from_millis(650))
        );
    }

    #[test]
    fn identifies_the_previous_local_model_before_switching() {
        let mut catalog = template_app::ProviderCatalog::default();
        catalog.select_whisper_provider();

        assert_eq!(
            Some(LocalModel::Whisper),
            lifecycle::active_local_model(&catalog)
        );
    }
}
