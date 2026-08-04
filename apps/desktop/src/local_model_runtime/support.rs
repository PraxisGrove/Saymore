use super::*;

pub(super) fn set_model_capability(ui: &AppWindow, model: LocalModel, available: bool) {
    match model {
        LocalModel::Paraformer => {
            ui.set_paraformer_download_supported(available);
            ui.set_paraformer_adapter_ready(available);
        }
        LocalModel::Whisper => {
            ui.set_whisper_download_supported(available);
            ui.set_whisper_adapter_ready(available);
        }
        LocalModel::Qwen3 => {
            ui.set_qwen3_asr_download_supported(available);
            ui.set_qwen3_asr_adapter_ready(available);
        }
        LocalModel::SenseVoice => {
            ui.set_sense_voice_download_supported(available);
            ui.set_sense_voice_adapter_ready(available);
        }
    }
}

pub(super) fn set_download_state(
    ui: &AppWindow,
    model: LocalModel,
    state: LocalModelDownloadState,
    progress: f32,
) {
    match model {
        LocalModel::Paraformer => {
            ui.set_paraformer_download_state(state);
            ui.set_paraformer_download_progress(progress);
        }
        LocalModel::Whisper => {
            ui.set_whisper_download_state(state);
            ui.set_whisper_download_progress(progress);
        }
        LocalModel::Qwen3 => {
            ui.set_qwen3_asr_download_state(state);
            ui.set_qwen3_asr_download_progress(progress);
        }
        LocalModel::SenseVoice => {
            ui.set_sense_voice_download_state(state);
            ui.set_sense_voice_download_progress(progress);
        }
    }
}

pub(super) fn set_download_progress(ui: &AppWindow, model: LocalModel, progress: f32) {
    match model {
        LocalModel::Paraformer => ui.set_paraformer_download_progress(progress),
        LocalModel::Whisper => ui.set_whisper_download_progress(progress),
        LocalModel::Qwen3 => ui.set_qwen3_asr_download_progress(progress),
        LocalModel::SenseVoice => ui.set_sense_voice_download_progress(progress),
    }
}

pub(super) fn download_progress(ui: &AppWindow, model: LocalModel) -> f32 {
    match model {
        LocalModel::Paraformer => ui.get_paraformer_download_progress(),
        LocalModel::Whisper => ui.get_whisper_download_progress(),
        LocalModel::Qwen3 => ui.get_qwen3_asr_download_progress(),
        LocalModel::SenseVoice => ui.get_sense_voice_download_progress(),
    }
}

pub(super) fn model_selected(ui: &AppWindow, model: LocalModel) -> bool {
    match model {
        LocalModel::Paraformer => ui.get_paraformer_selected(),
        LocalModel::Whisper => ui.get_whisper_selected(),
        LocalModel::Qwen3 => ui.get_qwen3_asr_selected(),
        LocalModel::SenseVoice => ui.get_sense_voice_selected(),
    }
}

pub(super) fn set_model_selected(ui: &AppWindow, model: LocalModel, selected: bool) {
    match model {
        LocalModel::Paraformer => ui.set_paraformer_selected(selected),
        LocalModel::Whisper => ui.set_whisper_selected(selected),
        LocalModel::Qwen3 => ui.set_qwen3_asr_selected(selected),
        LocalModel::SenseVoice => ui.set_sense_voice_selected(selected),
    }
}

pub(super) fn activation_pending(ui: &AppWindow, model: LocalModel) -> bool {
    match model {
        LocalModel::Paraformer => ui.get_paraformer_activation_pending(),
        LocalModel::Whisper => ui.get_whisper_activation_pending(),
        LocalModel::Qwen3 => ui.get_qwen3_asr_activation_pending(),
        LocalModel::SenseVoice => ui.get_sense_voice_activation_pending(),
    }
}

pub(super) fn set_activation_pending(ui: &AppWindow, model: LocalModel, pending: bool) {
    match model {
        LocalModel::Paraformer => ui.set_paraformer_activation_pending(pending),
        LocalModel::Whisper => ui.set_whisper_activation_pending(pending),
        LocalModel::Qwen3 => ui.set_qwen3_asr_activation_pending(pending),
        LocalModel::SenseVoice => ui.set_sense_voice_activation_pending(pending),
    }
}

pub(super) fn reset_model_ui(ui: &AppWindow, model: LocalModel) {
    set_download_state(ui, model, LocalModelDownloadState::NotDownloaded, 0.0);
    set_model_selected(ui, model, false);
    set_activation_pending(ui, model, false);
    set_runtime_memory(ui, model, false, SharedString::default());
    set_installed_size(ui, model, SharedString::default());
    ui.set_local_model_operation_error(SharedString::default());
}

pub(super) fn apply_model_sizes(
    ui: &AppWindow,
    model: LocalModel,
    installer: &VerifiedModelInstaller,
) -> Result<(), ModelInstallError> {
    set_download_size(
        ui,
        model,
        format_model_size(installer.download_size_bytes()).into(),
    );
    let installed_size = if installer.is_installed() {
        format_model_size(installer.installed_size_bytes()?).into()
    } else {
        SharedString::default()
    };
    set_installed_size(ui, model, installed_size);
    Ok(())
}

fn set_download_size(ui: &AppWindow, model: LocalModel, size: SharedString) {
    match model {
        LocalModel::Paraformer => ui.set_paraformer_download_size(size),
        LocalModel::Whisper => ui.set_whisper_download_size(size),
        LocalModel::Qwen3 => ui.set_qwen3_asr_download_size(size),
        LocalModel::SenseVoice => ui.set_sense_voice_download_size(size),
    }
}

fn set_installed_size(ui: &AppWindow, model: LocalModel, size: SharedString) {
    match model {
        LocalModel::Paraformer => ui.set_paraformer_installed_size(size),
        LocalModel::Whisper => ui.set_whisper_installed_size(size),
        LocalModel::Qwen3 => ui.set_qwen3_asr_installed_size(size),
        LocalModel::SenseVoice => ui.set_sense_voice_installed_size(size),
    }
}

pub(super) fn apply_download_failure(
    ui: &AppWindow,
    model: LocalModel,
    error: ModelInstallError,
    installed: bool,
) {
    ui.set_local_model_download_failure_kind(model.card_kind());
    set_download_state(
        ui,
        model,
        if installed {
            LocalModelDownloadState::Downloaded
        } else {
            LocalModelDownloadState::NotDownloaded
        },
        if installed { 1.0 } else { 0.0 },
    );
    ui.set_local_model_download_error(error.to_string().into());
    ui.set_local_model_download_space_failure(matches!(
        error,
        ModelInstallError::InsufficientSpace { .. }
    ));
    ui.set_local_model_download_failure_visible(true);
}

pub(super) fn apply_operation_failure(ui: &AppWindow, error: String) {
    ui.set_asr_test_succeeded(false);
    ui.set_asr_draft_error(true);
    ui.set_asr_test_result(SharedString::from(error));
}

pub(super) fn apply_activation_failure(ui: &AppWindow, model: LocalModel, error: String) {
    set_activation_pending(ui, model, false);
    ui.set_local_model_operation_error(error.into());
}

pub(super) fn apply_runtime_memory(ui: &AppWindow, model: LocalModel, memory_bytes: Option<u64>) {
    ui.set_local_model_operation_error(SharedString::default());
    let Some(memory_bytes) = memory_bytes else {
        set_runtime_memory(ui, model, false, SharedString::default());
        return;
    };
    set_runtime_memory(ui, model, true, format_memory_usage(memory_bytes).into());
}

pub(super) fn clear_runtime_memory(ui: &AppWindow, model: LocalModel) {
    set_runtime_memory(ui, model, false, SharedString::default());
}

fn set_runtime_memory(ui: &AppWindow, model: LocalModel, live: bool, usage: SharedString) {
    match model {
        LocalModel::Paraformer => {
            ui.set_paraformer_runtime_memory_live(live);
            ui.set_paraformer_runtime_memory_usage(usage);
        }
        LocalModel::Whisper => {
            ui.set_whisper_runtime_memory_live(live);
            ui.set_whisper_runtime_memory_usage(usage);
        }
        LocalModel::Qwen3 => {
            ui.set_qwen3_asr_runtime_memory_live(live);
            ui.set_qwen3_asr_runtime_memory_usage(usage);
        }
        LocalModel::SenseVoice => {
            ui.set_sense_voice_runtime_memory_live(live);
            ui.set_sense_voice_runtime_memory_usage(usage);
        }
    }
}

fn format_memory_usage(bytes: u64) -> String {
    const MEBIBYTE: f64 = 1_048_576.0;
    const GIBIBYTE: f64 = 1_073_741_824.0;
    if bytes >= GIBIBYTE as u64 {
        format!("{:.2} GB", bytes as f64 / GIBIBYTE)
    } else {
        format!("{:.0} MB", bytes as f64 / MEBIBYTE)
    }
}

pub(super) fn format_model_size(bytes: u64) -> String {
    const MEBIBYTE: f64 = 1_048_576.0;
    const GIBIBYTE: f64 = 1_073_741_824.0;
    if bytes >= GIBIBYTE as u64 {
        format!("{:.2} GB", bytes as f64 / GIBIBYTE)
    } else {
        format!("{:.0} MB", bytes as f64 / MEBIBYTE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_sampled_runtime_memory_for_the_ui() {
        assert_eq!("768 MB", format_memory_usage(768 * 1_048_576));
        assert_eq!("2.13 GB", format_memory_usage(2_287_072_256));
    }

    #[test]
    fn formats_model_bytes_with_unit_appropriate_precision() {
        assert_eq!("226 MB", format_model_size(237_202_501));
        assert_eq!("989 MB", format_model_size(1_036_613_791));
        assert_eq!("2.24 GB", format_model_size(2_404_222_421));
    }
}
