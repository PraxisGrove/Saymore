use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use slint::{ComponentHandle, SharedString};
use template_app::{
    InstalledModel, InstalledModelStore, ParaformerPunctuationMode, ProviderConfigStore,
};
use template_infra::{
    JsonSettingsStore, PUNCTUATION_MODEL_ID, PUNCTUATION_MODEL_REVISION, SqliteStorage,
    VerifiedModelInstaller,
};

use super::{format_model_size, runtime_error};
use crate::{
    asr_runtime::AsrSessionController,
    ui::{AppWindow, LocalModelDownloadState, PunctuationMode},
};

pub(super) fn wire(
    ui: &AppWindow,
    settings: Arc<JsonSettingsStore>,
    storage: Arc<SqliteStorage>,
    installer: Arc<VerifiedModelInstaller>,
    asr: Arc<AsrSessionController>,
) -> Result<(), io::Error> {
    let preload = apply_initial_state(ui, &settings, &storage, &installer)?;
    let busy = Arc::new(AtomicBool::new(false));
    wire_mode(
        ui,
        Arc::clone(&settings),
        Arc::clone(&installer),
        Arc::clone(&asr),
        Arc::clone(&busy),
    );
    wire_download(
        ui,
        Arc::clone(&settings),
        Arc::clone(&storage),
        Arc::clone(&installer),
        Arc::clone(&asr),
        Arc::clone(&busy),
    );
    wire_delete(
        ui,
        Arc::clone(&settings),
        storage,
        Arc::clone(&installer),
        Arc::clone(&asr),
        Arc::clone(&busy),
    );
    if preload {
        start_activation(ui, Arc::clone(&settings), asr, busy);
    }
    Ok(())
}

fn apply_initial_state(
    ui: &AppWindow,
    settings: &JsonSettingsStore,
    storage: &SqliteStorage,
    installer: &VerifiedModelInstaller,
) -> Result<bool, io::Error> {
    let installed = installer.is_installed();
    let mut catalog = settings.load_catalog().map_err(runtime_error)?;
    let requested_local = catalog.paraformer_punctuation_mode() == ParaformerPunctuationMode::Local;
    if requested_local && !installed {
        catalog.set_paraformer_punctuation_mode(ParaformerPunctuationMode::Llm);
        settings.save_catalog(&catalog).map_err(runtime_error)?;
    }
    ui.set_punctuation_mode(if requested_local && installed {
        PunctuationMode::Local
    } else {
        PunctuationMode::Llm
    });
    ui.set_punctuation_download_state(if installed {
        LocalModelDownloadState::Downloaded
    } else {
        LocalModelDownloadState::NotDownloaded
    });
    ui.set_punctuation_download_progress(if installed { 1.0 } else { 0.0 });
    ui.set_punctuation_download_size(format_model_size(installer.download_size_bytes()).into());
    ui.set_punctuation_installed_size(if installed {
        format_model_size(installer.installed_size_bytes().map_err(runtime_error)?).into()
    } else {
        SharedString::default()
    });
    if installed {
        reconcile_installed_model(storage, &installer.model_directory()).map_err(runtime_error)?;
    } else {
        storage
            .delete_installed_model(PUNCTUATION_MODEL_ID)
            .map_err(runtime_error)?;
    }
    Ok(requested_local && installed && catalog.paraformer_is_active())
}

fn wire_mode(
    ui: &AppWindow,
    settings: Arc<JsonSettingsStore>,
    installer: Arc<VerifiedModelInstaller>,
    asr: Arc<AsrSessionController>,
    busy: Arc<AtomicBool>,
) {
    let weak = ui.as_weak();
    ui.on_request_punctuation_mode(move |mode| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        ui.set_punctuation_error(SharedString::default());
        match mode {
            PunctuationMode::Llm => {
                if save_mode(&settings, ParaformerPunctuationMode::Llm).is_ok() {
                    asr.clear_punctuation();
                    ui.set_punctuation_mode(PunctuationMode::Llm);
                    ui.set_punctuation_runtime_live(false);
                    ui.set_punctuation_runtime_memory(SharedString::default());
                }
            }
            PunctuationMode::Local => {
                ui.set_punctuation_mode(PunctuationMode::Local);
                if installer.is_installed() {
                    start_activation(
                        &ui,
                        Arc::clone(&settings),
                        Arc::clone(&asr),
                        Arc::clone(&busy),
                    );
                }
            }
        }
    });
}

fn wire_download(
    ui: &AppWindow,
    settings: Arc<JsonSettingsStore>,
    storage: Arc<SqliteStorage>,
    installer: Arc<VerifiedModelInstaller>,
    asr: Arc<AsrSessionController>,
    busy: Arc<AtomicBool>,
) {
    let weak = ui.as_weak();
    ui.on_request_punctuation_download(move || {
        if busy.swap(true, Ordering::AcqRel) {
            return;
        }
        let Some(ui) = weak.upgrade() else {
            busy.store(false, Ordering::Release);
            return;
        };
        ui.set_punctuation_download_state(LocalModelDownloadState::Downloading);
        ui.set_punctuation_error(SharedString::default());
        let result_ui = weak.clone();
        let progress_ui = weak.clone();
        let settings = Arc::clone(&settings);
        let storage = Arc::clone(&storage);
        let installer = Arc::clone(&installer);
        let asr = Arc::clone(&asr);
        let busy_thread = Arc::clone(&busy);
        let spawn = std::thread::Builder::new()
            .name("saymore-punctuation-download".to_owned())
            .spawn(move || {
                let result =
                    download_and_activate(&settings, &storage, &installer, &asr, progress_ui);
                let installed = installer.is_installed();
                busy_thread.store(false, Ordering::Release);
                let _ =
                    result_ui.upgrade_in_event_loop(move |ui| apply_result(&ui, result, installed));
            });
        if let Err(error) = spawn {
            busy.store(false, Ordering::Release);
            ui.set_punctuation_download_state(LocalModelDownloadState::NotDownloaded);
            ui.set_punctuation_error(error.to_string().into());
        }
    });
}

fn download_and_activate(
    settings: &JsonSettingsStore,
    storage: &SqliteStorage,
    installer: &VerifiedModelInstaller,
    asr: &AsrSessionController,
    progress_ui: slint::Weak<AppWindow>,
) -> Result<(SharedString, bool, Option<u64>), String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    let progress = Arc::new(move |progress: template_infra::ModelDownloadProgress| {
        let ratio = if progress.total_bytes == 0 {
            0.0
        } else {
            progress.downloaded_bytes as f32 / progress.total_bytes as f32
        };
        let _ = progress_ui
            .upgrade_in_event_loop(move |ui| ui.set_punctuation_download_progress(ratio));
    });
    let path = runtime
        .block_on(installer.install(progress))
        .map_err(|error| error.to_string())?;
    save_installed_model(storage, &path).map_err(|error| error.to_string())?;
    let paraformer_active = settings
        .load_catalog()
        .map_err(|error| error.to_string())?
        .paraformer_is_active();
    let memory = if paraformer_active {
        asr.prepare_punctuation()
            .map_err(|error| error.to_string())?
    } else {
        None
    };
    save_mode(settings, ParaformerPunctuationMode::Local)?;
    let size = format_model_size(
        installer
            .installed_size_bytes()
            .map_err(|error| error.to_string())?,
    )
    .into();
    Ok((size, paraformer_active, memory))
}

fn start_activation(
    ui: &AppWindow,
    settings: Arc<JsonSettingsStore>,
    asr: Arc<AsrSessionController>,
    busy: Arc<AtomicBool>,
) {
    if busy.swap(true, Ordering::AcqRel) {
        return;
    }
    ui.set_punctuation_pending(true);
    let weak = ui.as_weak();
    let spawn_busy = Arc::clone(&busy);
    let spawn = std::thread::Builder::new()
        .name("saymore-punctuation-activate".to_owned())
        .spawn(move || {
            let result = settings
                .load_catalog()
                .map_err(|error| error.to_string())
                .and_then(|catalog| {
                    let paraformer_active = catalog.paraformer_is_active();
                    let memory = if paraformer_active {
                        asr.prepare_punctuation()
                            .map_err(|error| error.to_string())?
                    } else {
                        None
                    };
                    save_mode(&settings, ParaformerPunctuationMode::Local)?;
                    Ok((SharedString::default(), paraformer_active, memory))
                });
            spawn_busy.store(false, Ordering::Release);
            let _ = weak.upgrade_in_event_loop(move |ui| apply_result(&ui, result, true));
        });
    if let Err(error) = spawn {
        busy.store(false, Ordering::Release);
        ui.set_punctuation_pending(false);
        ui.set_punctuation_error(error.to_string().into());
    }
}

fn apply_result(
    ui: &AppWindow,
    result: Result<(SharedString, bool, Option<u64>), String>,
    installed: bool,
) {
    ui.set_punctuation_pending(false);
    match result {
        Ok((installed_size, loaded, memory)) => {
            ui.set_punctuation_mode(PunctuationMode::Local);
            ui.set_punctuation_download_state(LocalModelDownloadState::Downloaded);
            ui.set_punctuation_download_progress(1.0);
            if !installed_size.is_empty() {
                ui.set_punctuation_installed_size(installed_size);
            }
            ui.set_punctuation_runtime_live(loaded);
            ui.set_punctuation_runtime_memory(if loaded {
                memory.map(format_memory).unwrap_or_default().into()
            } else {
                SharedString::default()
            });
            ui.set_punctuation_error(SharedString::default());
            ui.invoke_refresh_usage();
        }
        Err(error) => {
            ui.set_punctuation_download_state(if installed {
                LocalModelDownloadState::Downloaded
            } else {
                LocalModelDownloadState::NotDownloaded
            });
            ui.set_punctuation_error(error.into());
        }
    }
}

fn wire_delete(
    ui: &AppWindow,
    settings: Arc<JsonSettingsStore>,
    storage: Arc<SqliteStorage>,
    installer: Arc<VerifiedModelInstaller>,
    asr: Arc<AsrSessionController>,
    busy: Arc<AtomicBool>,
) {
    let weak = ui.as_weak();
    ui.on_request_punctuation_delete(move || {
        if busy.swap(true, Ordering::AcqRel) {
            return;
        }
        asr.clear_punctuation();
        let result = installer
            .remove()
            .map_err(|error| error.to_string())
            .and_then(|()| {
                storage
                    .delete_installed_model(PUNCTUATION_MODEL_ID)
                    .map_err(|error| error.to_string())
            })
            .and_then(|()| save_mode(&settings, ParaformerPunctuationMode::Llm));
        busy.store(false, Ordering::Release);
        if let Some(ui) = weak.upgrade() {
            match result {
                Ok(()) => {
                    ui.set_punctuation_mode(PunctuationMode::Llm);
                    ui.set_punctuation_download_state(LocalModelDownloadState::NotDownloaded);
                    ui.set_punctuation_download_progress(0.0);
                    ui.set_punctuation_installed_size(SharedString::default());
                    ui.set_punctuation_runtime_live(false);
                    ui.set_punctuation_runtime_memory(SharedString::default());
                    ui.invoke_refresh_usage();
                }
                Err(error) => ui.set_punctuation_error(error.into()),
            }
        }
    });
}

fn save_mode(settings: &JsonSettingsStore, mode: ParaformerPunctuationMode) -> Result<(), String> {
    let mut catalog = settings.load_catalog().map_err(|error| error.to_string())?;
    catalog.set_paraformer_punctuation_mode(mode);
    settings
        .save_catalog(&catalog)
        .map_err(|error| error.to_string())
}

fn save_installed_model(
    storage: &SqliteStorage,
    path: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let path = path
        .to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "model path is not Unicode"))?;
    let installed_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_millis()
        .try_into()?;
    storage.save_installed_model(InstalledModel {
        id: PUNCTUATION_MODEL_ID.to_owned(),
        provider_type: "local_punctuation".to_owned(),
        model: "CT-Transformer zh-en punctuation INT8".to_owned(),
        version: PUNCTUATION_MODEL_REVISION.to_owned(),
        path: path.to_owned(),
        installed_at_ms,
        last_verified_at_ms: Some(installed_at_ms),
    })?;
    Ok(())
}

fn reconcile_installed_model(
    storage: &SqliteStorage,
    path: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if storage
        .list_installed_models()?
        .iter()
        .any(|model| model.id == PUNCTUATION_MODEL_ID)
    {
        Ok(())
    } else {
        save_installed_model(storage, path)
    }
}

fn format_memory(bytes: u64) -> String {
    format!("{:.0} MB", bytes as f64 / 1_048_576.0)
}
