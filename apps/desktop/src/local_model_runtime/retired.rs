use std::{
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

use slint::{ComponentHandle, SharedString};
use template_app::{InstalledModelStore, ModelDownloadQueueStore};
use template_infra::{RetiredAsrModel, SqliteStorage};

use super::{format_model_size, runtime_error};
use crate::ui::{AppWindow, LocalModelDownloadState};

pub(super) fn wire(
    ui: &AppWindow,
    storage: Arc<SqliteStorage>,
    root: PathBuf,
) -> Result<(), io::Error> {
    for model in RetiredAsrModel::ALL {
        apply_state(ui, model, &root)?;
    }
    let weak = ui.as_weak();
    ui.on_request_punctuation_delete(move || {
        if let Some(ui) = weak.upgrade() {
            remove(&ui, &storage, &root, RetiredAsrModel::Punctuation);
        }
    });
    Ok(())
}

pub(super) fn remove(ui: &AppWindow, storage: &SqliteStorage, root: &Path, model: RetiredAsrModel) {
    ui.set_local_model_operation_error(SharedString::default());
    let result = model
        .remove_files(root)
        .map_err(runtime_error)
        .and_then(|()| {
            storage
                .delete_installed_model(model.id())
                .map_err(runtime_error)
        })
        .and_then(|()| {
            storage
                .remove_model_download(model.id())
                .map_err(runtime_error)
        })
        .and_then(|()| apply_state(ui, model, root));
    if let Err(error) = result {
        ui.set_local_model_operation_error(error.to_string().into());
    }
    ui.invoke_refresh_usage();
}

fn apply_state(ui: &AppWindow, model: RetiredAsrModel, root: &Path) -> Result<(), io::Error> {
    let bytes = model.storage_bytes(root).map_err(runtime_error)?;
    let state = if bytes > 0 {
        LocalModelDownloadState::Downloaded
    } else {
        LocalModelDownloadState::NotDownloaded
    };
    let size = format_model_size(bytes).into();
    match model {
        RetiredAsrModel::Paraformer => {
            ui.set_paraformer_download_state(state);
            ui.set_paraformer_installed_size(size);
            ui.set_paraformer_activation_pending(false);
        }
        RetiredAsrModel::Punctuation => {
            ui.set_punctuation_download_state(state);
            ui.set_punctuation_installed_size(size);
            ui.set_punctuation_pending(false);
        }
        RetiredAsrModel::SenseVoice => {
            ui.set_sense_voice_download_state(state);
            ui.set_sense_voice_installed_size(size);
        }
        RetiredAsrModel::Moonshine => {
            ui.set_moonshine_download_state(state);
            ui.set_moonshine_installed_size(size);
        }
    }
    Ok(())
}
