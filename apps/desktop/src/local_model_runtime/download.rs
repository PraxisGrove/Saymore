use std::collections::HashMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

use slint::{ComponentHandle, SharedString};
use template_infra::{
    ModelDownloadProgress, ModelInstallControl, ModelInstallError, ModelInstallInterruption,
    SqliteStorage, VerifiedModelInstaller,
};

use super::{
    LocalModel, LocalModelInstallers, apply_download_failure, apply_model_size_strings,
    apply_model_sizes_result, download_progress, installer_for, lifecycle::record_installation,
    set_download_progress, set_download_state,
};
use crate::ui::{AppWindow, LocalModelDownloadState};

struct ActiveDownload {
    generation: u64,
    control: ModelInstallControl,
}

#[derive(Default)]
struct DownloadCoordinator {
    active: Mutex<HashMap<LocalModel, ActiveDownload>>,
    next_generation: AtomicU64,
}

impl DownloadCoordinator {
    fn begin(&self, model: LocalModel) -> Option<(u64, ModelInstallControl)> {
        let Ok(mut active) = self.active.lock() else {
            return None;
        };
        if active.contains_key(&model) {
            return None;
        }
        let generation = self.next_generation.fetch_add(1, Ordering::AcqRel);
        let control = ModelInstallControl::default();
        active.insert(
            model,
            ActiveDownload {
                generation,
                control: control.clone(),
            },
        );
        Some((generation, control))
    }

    fn interrupt(&self, model: LocalModel, interruption: ModelInstallInterruption) -> bool {
        let control = {
            let Ok(active) = self.active.lock() else {
                return false;
            };
            active.get(&model).map(|download| download.control.clone())
        };
        let Some(control) = control else {
            return false;
        };
        match interruption {
            ModelInstallInterruption::Paused => control.pause(),
            ModelInstallInterruption::Cancelled => control.cancel(),
        }
        true
    }

    fn is_current(&self, model: LocalModel, generation: u64) -> bool {
        self.active.lock().is_ok_and(|active| {
            active
                .get(&model)
                .is_some_and(|download| download.generation == generation)
        })
    }

    fn finish(&self, model: LocalModel, generation: u64) -> bool {
        let Ok(mut active) = self.active.lock() else {
            return false;
        };
        if active
            .get(&model)
            .is_none_or(|download| download.generation != generation)
        {
            return false;
        }
        active.remove(&model);
        true
    }
}

enum DownloadCompletion {
    Installed(Result<(SharedString, SharedString), ModelInstallError>),
    Paused,
    Cancelled,
    Failed(ModelInstallError, bool),
}

pub(super) fn wire_download(
    ui: &AppWindow,
    storage: Arc<SqliteStorage>,
    installers: Arc<LocalModelInstallers>,
) {
    let downloads = Arc::new(DownloadCoordinator::default());
    wire_start(ui, Arc::clone(&downloads), storage, Arc::clone(&installers));
    wire_pause(ui, Arc::clone(&downloads), Arc::clone(&installers));
    wire_cancel(ui, downloads, installers);
}

fn wire_start(
    ui: &AppWindow,
    downloads: Arc<DownloadCoordinator>,
    storage: Arc<SqliteStorage>,
    installers: Arc<LocalModelInstallers>,
) {
    let weak = ui.as_weak();
    ui.on_request_local_model_download(move |kind| {
        let Some((model, installer)) = installer_for(kind, &installers) else {
            return;
        };
        let Some((generation, control)) = downloads.begin(model) else {
            return;
        };
        let Some(ui) = weak.upgrade() else {
            let _ = downloads.finish(model, generation);
            return;
        };
        let progress = download_progress(&ui, model);
        set_download_state(&ui, model, LocalModelDownloadState::Downloading, progress);
        ui.set_local_model_download_failure_visible(false);
        ui.set_local_model_download_space_failure(false);
        ui.set_local_model_download_error(SharedString::default());
        ui.set_local_model_operation_error(SharedString::default());

        let result_ui = weak.clone();
        let progress_ui = weak.clone();
        let progress_downloads = Arc::clone(&downloads);
        let result_downloads = Arc::clone(&downloads);
        let storage = Arc::clone(&storage);
        let spawn = std::thread::Builder::new()
            .name(model.thread_name("download"))
            .spawn(move || {
                let last_per_mille = Arc::new(AtomicU64::new(u64::MAX));
                let progress = Arc::new(move |progress: ModelDownloadProgress| {
                    if !progress_downloads.is_current(model, generation) {
                        return;
                    }
                    let per_mille = progress_per_mille(progress);
                    if last_per_mille.swap(per_mille, Ordering::AcqRel) == per_mille {
                        return;
                    }
                    let current_downloads = Arc::clone(&progress_downloads);
                    let _ = progress_ui.upgrade_in_event_loop(move |ui| {
                        if current_downloads.is_current(model, generation) {
                            set_download_progress(&ui, model, per_mille as f32 / 1_000.0);
                        }
                    });
                });
                let completion = run_download(model, &installer, &storage, progress, control);
                if !result_downloads.finish(model, generation) {
                    return;
                }
                let _ = result_ui.upgrade_in_event_loop(move |ui| {
                    apply_completion(&ui, model, completion);
                });
            });
        if let Err(error) = spawn {
            let _ = downloads.finish(model, generation);
            apply_download_failure(
                &ui,
                model,
                ModelInstallError::Filesystem(error.to_string()),
                false,
            );
        }
    });
}

fn wire_pause(
    ui: &AppWindow,
    downloads: Arc<DownloadCoordinator>,
    installers: Arc<LocalModelInstallers>,
) {
    let weak = ui.as_weak();
    ui.on_request_local_model_pause(move |kind| {
        let Some((model, _)) = installer_for(kind, &installers) else {
            return;
        };
        if !downloads.interrupt(model, ModelInstallInterruption::Paused) {
            return;
        }
        if let Some(ui) = weak.upgrade() {
            let progress = download_progress(&ui, model);
            set_download_state(&ui, model, LocalModelDownloadState::Paused, progress);
        }
    });
}

fn wire_cancel(
    ui: &AppWindow,
    downloads: Arc<DownloadCoordinator>,
    installers: Arc<LocalModelInstallers>,
) {
    let weak = ui.as_weak();
    ui.on_request_local_model_cancel_download(move |kind| {
        let Some((model, installer)) = installer_for(kind, &installers) else {
            return;
        };
        if downloads.interrupt(model, ModelInstallInterruption::Cancelled) {
            if let Some(ui) = weak.upgrade() {
                let progress = download_progress(&ui, model);
                set_download_state(&ui, model, LocalModelDownloadState::Cancelling, progress);
            }
            return;
        }
        let Some((generation, _)) = downloads.begin(model) else {
            return;
        };
        let Some(ui) = weak.upgrade() else {
            let _ = downloads.finish(model, generation);
            return;
        };
        let progress = download_progress(&ui, model);
        set_download_state(&ui, model, LocalModelDownloadState::Cancelling, progress);
        let result_ui = weak.clone();
        let result_downloads = Arc::clone(&downloads);
        let spawn = std::thread::Builder::new()
            .name(model.thread_name("cancel-download"))
            .spawn(move || {
                let completion = match installer.discard_partial_download() {
                    Ok(()) => DownloadCompletion::Cancelled,
                    Err(error) => DownloadCompletion::Failed(error, false),
                };
                if !result_downloads.finish(model, generation) {
                    return;
                }
                let _ = result_ui.upgrade_in_event_loop(move |ui| {
                    apply_completion(&ui, model, completion);
                });
            });
        if let Err(error) = spawn {
            let _ = downloads.finish(model, generation);
            apply_download_failure(
                &ui,
                model,
                ModelInstallError::Filesystem(error.to_string()),
                false,
            );
        }
    });
}

fn run_download(
    model: LocalModel,
    installer: &VerifiedModelInstaller,
    storage: &SqliteStorage,
    progress: Arc<dyn Fn(ModelDownloadProgress) + Send + Sync>,
    control: ModelInstallControl,
) -> DownloadCompletion {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            return DownloadCompletion::Failed(
                ModelInstallError::Filesystem(error.to_string()),
                false,
            );
        }
    };
    match runtime.block_on(installer.install_with_control(progress, control)) {
        Ok(path) => match record_installation(storage, model, &path) {
            Ok(()) => DownloadCompletion::Installed(apply_model_sizes_result(installer)),
            Err(error) => DownloadCompletion::Failed(
                ModelInstallError::Activation(error.to_string()),
                installer.is_installed(),
            ),
        },
        Err(ModelInstallError::Interrupted(ModelInstallInterruption::Paused)) => {
            DownloadCompletion::Paused
        }
        Err(ModelInstallError::Interrupted(ModelInstallInterruption::Cancelled)) => {
            match installer.discard_partial_download() {
                Ok(()) => DownloadCompletion::Cancelled,
                Err(error) => DownloadCompletion::Failed(error, false),
            }
        }
        Err(error) => DownloadCompletion::Failed(error, installer.is_installed()),
    }
}

fn apply_completion(ui: &AppWindow, model: LocalModel, completion: DownloadCompletion) {
    match completion {
        DownloadCompletion::Installed(model_sizes) => {
            set_download_state(ui, model, LocalModelDownloadState::Downloaded, 1.0);
            ui.set_local_model_download_error(SharedString::default());
            match model_sizes {
                Ok((download_size, installed_size)) => {
                    apply_model_size_strings(ui, model, download_size, installed_size);
                }
                Err(error) => {
                    tracing::warn!(
                        event = "model.installed_size_measurement_failed",
                        model_id = model.id(),
                        reason = %error
                    );
                }
            }
            ui.invoke_refresh_usage();
        }
        DownloadCompletion::Paused => {
            let progress = download_progress(ui, model);
            set_download_state(ui, model, LocalModelDownloadState::Paused, progress);
        }
        DownloadCompletion::Cancelled => {
            set_download_state(ui, model, LocalModelDownloadState::NotDownloaded, 0.0);
            ui.set_local_model_download_error(SharedString::default());
            ui.invoke_refresh_usage();
        }
        DownloadCompletion::Failed(error, installed) => {
            apply_download_failure(ui, model, error, installed);
        }
    }
}

fn progress_per_mille(progress: ModelDownloadProgress) -> u64 {
    progress
        .downloaded_bytes
        .saturating_mul(1_000)
        .checked_div(progress.total_bytes)
        .unwrap_or(0)
        .min(1_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coordinator_allows_only_one_active_task_per_model() {
        let coordinator = DownloadCoordinator::default();
        let Some((generation, _)) = coordinator.begin(LocalModel::Whisper) else {
            panic!("first download should start");
        };

        assert!(coordinator.begin(LocalModel::Whisper).is_none());
        assert!(coordinator.begin(LocalModel::Qwen3).is_some());
        assert!(coordinator.finish(LocalModel::Whisper, generation));
        assert!(coordinator.begin(LocalModel::Whisper).is_some());
    }

    #[test]
    fn stale_completion_cannot_remove_a_newer_task() {
        let coordinator = DownloadCoordinator::default();
        let Some((generation, _)) = coordinator.begin(LocalModel::Paraformer) else {
            panic!("download should start");
        };

        assert!(!coordinator.finish(LocalModel::Paraformer, generation + 1));
        assert!(coordinator.is_current(LocalModel::Paraformer, generation));
    }

    #[test]
    fn progress_conversion_is_bounded_and_handles_an_empty_manifest() {
        assert_eq!(
            0,
            progress_per_mille(ModelDownloadProgress {
                downloaded_bytes: 10,
                total_bytes: 0,
            })
        );
        assert_eq!(
            1_000,
            progress_per_mille(ModelDownloadProgress {
                downloaded_bytes: 11,
                total_bytes: 10,
            })
        );
    }
}
