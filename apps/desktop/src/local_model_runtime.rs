use std::{
    io,
    sync::Arc,
    time::{Duration, Instant},
};

use slint::{ComponentHandle, SharedString};
use template_app::ProviderConfigStore;
use template_infra::{JsonSettingsStore, ModelInstallError, SqliteStorage, VerifiedModelInstaller};

use crate::{
    asr_runtime::{AsrSessionController, current_process_resident_memory_bytes},
    settings_ui,
    ui::{AppWindow, AsrProviderCardKind, LocalModelDownloadState},
};

mod download;
mod lifecycle;
mod model;
mod punctuation;
mod recognition_test;
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
    paraformer: Arc<VerifiedModelInstaller>,
    whisper: Arc<VerifiedModelInstaller>,
    qwen3: Arc<VerifiedModelInstaller>,
    sense_voice: Arc<VerifiedModelInstaller>,
}

impl LocalModelInstallers {
    fn for_model(&self, model: LocalModel) -> Arc<VerifiedModelInstaller> {
        match model {
            LocalModel::Paraformer => Arc::clone(&self.paraformer),
            LocalModel::Whisper => Arc::clone(&self.whisper),
            LocalModel::Qwen3 => Arc::clone(&self.qwen3),
            LocalModel::SenseVoice => Arc::clone(&self.sense_voice),
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
    let installers = Arc::new(LocalModelInstallers {
        paraformer: Arc::new(
            VerifiedModelInstaller::paraformer(models_directory.clone()).map_err(runtime_error)?,
        ),
        whisper: Arc::new(
            VerifiedModelInstaller::whisper_large_v3_turbo(models_directory.clone())
                .map_err(runtime_error)?,
        ),
        qwen3: Arc::new(
            VerifiedModelInstaller::qwen3_asr_1_7b(models_directory.clone())
                .map_err(runtime_error)?,
        ),
        sense_voice: Arc::new(
            VerifiedModelInstaller::sense_voice_small(models_directory.clone())
                .map_err(runtime_error)?,
        ),
    });
    let punctuation =
        Arc::new(VerifiedModelInstaller::punctuation(models_directory).map_err(runtime_error)?);
    for model in [
        LocalModel::Paraformer,
        LocalModel::Whisper,
        LocalModel::Qwen3,
        LocalModel::SenseVoice,
    ] {
        let installer = installers.for_model(model);
        apply_state(ui, &settings, &storage, model, &installer)?;
    }
    wire_download(ui, Arc::clone(&storage), Arc::clone(&installers));
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
    );
    wire_test(ui, Arc::clone(&settings), installers, Arc::clone(&asr));
    punctuation::wire(
        ui,
        Arc::clone(&settings),
        Arc::clone(&storage),
        punctuation,
        Arc::clone(&asr),
    )?;
    for model in [
        LocalModel::Paraformer,
        LocalModel::Whisper,
        LocalModel::Qwen3,
        LocalModel::SenseVoice,
    ] {
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
        for model in inactive_local_models(
            ui.get_paraformer_selected(),
            ui.get_whisper_selected(),
            ui.get_qwen3_asr_selected(),
            ui.get_sense_voice_selected(),
        ) {
            clear(asr.as_ref(), model);
            clear_runtime_memory(&ui, model);
        }
        if !ui.get_paraformer_selected() {
            asr.clear_punctuation();
            ui.set_punctuation_runtime_live(false);
            ui.set_punctuation_runtime_memory(SharedString::default());
        }
    });
}

fn inactive_local_models(
    paraformer_selected: bool,
    whisper_selected: bool,
    qwen3_selected: bool,
    sense_voice_selected: bool,
) -> impl Iterator<Item = LocalModel> {
    [
        (LocalModel::Paraformer, paraformer_selected),
        (LocalModel::Whisper, whisper_selected),
        (LocalModel::Qwen3, qwen3_selected),
        (LocalModel::SenseVoice, sense_voice_selected),
    ]
    .into_iter()
    .filter_map(|(model, selected)| (!selected).then_some(model))
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
                    .map(|activation| {
                        let punctuation_memory =
                            prepare_punctuation_if_configured(&settings, &asr, model);
                        (activation.memory_bytes, punctuation_memory)
                    });
                std::thread::sleep(remaining_activation_feedback(feedback_started.elapsed()));
                let event_settings = Arc::clone(&settings);
                let _ = result_ui.upgrade_in_event_loop(move |ui| match result {
                    Ok((memory_bytes, punctuation_memory)) => {
                        settings_ui::reload_settings(&ui, &event_settings);
                        apply_runtime_memory(
                            &ui,
                            model,
                            current_process_resident_memory_bytes().or(memory_bytes),
                        );
                        if model == LocalModel::Paraformer && punctuation_memory.is_some() {
                            ui.set_punctuation_runtime_live(true);
                            ui.set_punctuation_runtime_memory(
                                punctuation_memory
                                    .flatten()
                                    .map(format_runtime_memory)
                                    .unwrap_or_default()
                                    .into(),
                            );
                        }
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

fn prepare_punctuation_if_configured(
    settings: &JsonSettingsStore,
    asr: &AsrSessionController,
    model: LocalModel,
) -> Option<Option<u64>> {
    let local_mode = model == LocalModel::Paraformer
        && settings.load_catalog().is_ok_and(|catalog| {
            catalog.paraformer_punctuation_mode() == template_app::ParaformerPunctuationMode::Local
        });
    if !local_mode {
        return None;
    }
    match asr.prepare_punctuation() {
        Ok(memory) => Some(memory),
        Err(error) => {
            tracing::warn!(
                target: "saymore::diagnostics",
                event = "punctuation.preload_failed",
                reason = %error
            );
            None
        }
    }
}

fn wire_delete(
    ui: &AppWindow,
    settings: Arc<JsonSettingsStore>,
    storage: Arc<SqliteStorage>,
    installers: Arc<LocalModelInstallers>,
    asr: Arc<AsrSessionController>,
) {
    let weak = ui.as_weak();
    ui.on_request_local_model_delete(move |kind| {
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

fn format_runtime_memory(bytes: u64) -> String {
    format!("{:.0} MB", bytes as f64 / 1_048_576.0)
}

fn runtime_error(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inactive_local_models_are_released_after_provider_selection() {
        assert_eq!(
            vec![
                LocalModel::Whisper,
                LocalModel::Qwen3,
                LocalModel::SenseVoice
            ],
            inactive_local_models(true, false, false, false).collect::<Vec<_>>()
        );
        assert_eq!(
            vec![
                LocalModel::Paraformer,
                LocalModel::Whisper,
                LocalModel::Qwen3,
                LocalModel::SenseVoice
            ],
            inactive_local_models(false, false, false, false).collect::<Vec<_>>()
        );
    }

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
