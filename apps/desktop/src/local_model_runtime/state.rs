use super::*;

pub(super) fn apply_state(
    ui: &AppWindow,
    settings: &JsonSettingsStore,
    storage: &SqliteStorage,
    model: LocalModel,
    installer: &VerifiedModelInstaller,
) -> Result<(), io::Error> {
    let installed = installer.is_installed();
    let partial_progress = installer
        .partial_download_progress()
        .map_err(runtime_error)?;
    let partial = !installed && partial_progress.downloaded_bytes > 0;
    let installed_path = installed.then(|| installer.model_directory());
    let reconciled = lifecycle::reconcile(settings, storage, model, installed_path.as_deref())
        .map_err(runtime_error)?;
    if reconciled.provider_selection_changed {
        settings_ui::reload_settings(ui, settings);
    }
    set_model_capability(ui, model, true);
    apply_model_sizes(ui, model, installer).map_err(runtime_error)?;
    set_download_state(
        ui,
        model,
        if installed {
            LocalModelDownloadState::Downloaded
        } else if partial {
            LocalModelDownloadState::Paused
        } else {
            LocalModelDownloadState::NotDownloaded
        },
        if installed {
            1.0
        } else if partial_progress.total_bytes == 0 {
            0.0
        } else {
            partial_progress.downloaded_bytes as f32 / partial_progress.total_bytes as f32
        },
    );
    set_model_selected(ui, model, reconciled.selected);
    Ok(())
}

pub(super) fn apply_model_sizes_result(
    installer: &VerifiedModelInstaller,
) -> Result<(SharedString, SharedString), ModelInstallError> {
    let download_size = format_model_size(installer.download_size_bytes()).into();
    let installed_size = format_model_size(installer.installed_size_bytes()?).into();
    Ok((download_size, installed_size))
}

pub(super) fn apply_model_size_strings(
    ui: &AppWindow,
    model: LocalModel,
    download_size: SharedString,
    installed_size: SharedString,
) {
    match model {
        LocalModel::Paraformer => {
            ui.set_paraformer_download_size(download_size);
            ui.set_paraformer_installed_size(installed_size);
        }
        LocalModel::Whisper => {
            ui.set_whisper_download_size(download_size);
            ui.set_whisper_installed_size(installed_size);
        }
        LocalModel::Qwen3 => {
            ui.set_qwen3_asr_download_size(download_size);
            ui.set_qwen3_asr_installed_size(installed_size);
        }
        LocalModel::SenseVoice => {
            ui.set_sense_voice_download_size(download_size);
            ui.set_sense_voice_installed_size(installed_size);
        }
    }
}
