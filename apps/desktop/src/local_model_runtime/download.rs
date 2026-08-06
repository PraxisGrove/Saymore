use std::collections::{HashMap, VecDeque};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

use slint::{ComponentHandle, SharedString};
use template_app::{ModelDownloadQueueStore, QueuedModelDownload};
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
    state: Mutex<DownloadCoordinatorState>,
    next_generation: AtomicU64,
}

#[derive(Default)]
struct DownloadCoordinatorState {
    active: HashMap<LocalModel, ActiveDownload>,
    queued: VecDeque<LocalModel>,
}

const MAX_ACTIVE_DOWNLOADS: usize = 2;

impl DownloadCoordinator {
    fn enqueue(&self, model: LocalModel) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if !state.active.contains_key(&model) && !state.queued.contains(&model) {
            state.queued.push_back(model);
        }
    }

    fn take_ready(&self) -> Vec<(LocalModel, u64, ModelInstallControl)> {
        let Ok(mut state) = self.state.lock() else {
            return Vec::new();
        };
        let available = MAX_ACTIVE_DOWNLOADS.saturating_sub(state.active.len());
        let mut ready = Vec::with_capacity(available);
        for _ in 0..available {
            let Some(model) = state.queued.pop_front() else {
                break;
            };
            let generation = self.next_generation.fetch_add(1, Ordering::AcqRel);
            let control = ModelInstallControl::default();
            state.active.insert(
                model,
                ActiveDownload {
                    generation,
                    control: control.clone(),
                },
            );
            ready.push((model, generation, control));
        }
        ready
    }

    fn interrupt(&self, model: LocalModel, interruption: ModelInstallInterruption) -> bool {
        let control = {
            let Ok(state) = self.state.lock() else {
                return false;
            };
            state
                .active
                .get(&model)
                .map(|download| download.control.clone())
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
        self.state.lock().is_ok_and(|state| {
            state
                .active
                .get(&model)
                .is_some_and(|download| download.generation == generation)
        })
    }

    fn finish(&self, model: LocalModel, generation: u64) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if state
            .active
            .get(&model)
            .is_none_or(|download| download.generation != generation)
        {
            return false;
        }
        state.active.remove(&model);
        true
    }

    fn remove_queued(&self, model: LocalModel) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        let Some(index) = state.queued.iter().position(|queued| *queued == model) else {
            return false;
        };
        state.queued.remove(index);
        true
    }
}

enum DownloadCompletion {
    Installed(Result<(SharedString, SharedString), ModelInstallError>),
    Paused,
    Cancelled,
    Failed(ModelInstallError, bool),
}

impl DownloadCompletion {
    fn clears_persisted_queue_entry(&self) -> bool {
        matches!(self, Self::Installed(_) | Self::Cancelled)
    }
}

pub(super) fn wire_download(
    ui: &AppWindow,
    storage: Arc<SqliteStorage>,
    installers: Arc<LocalModelInstallers>,
) {
    let downloads = Arc::new(DownloadCoordinator::default());
    restore_queue(ui, storage.as_ref(), installers.as_ref());
    let runtime = Arc::new(DownloadRuntime {
        ui: ui.as_weak(),
        downloads,
        storage,
        installers,
    });
    wire_start(ui, Arc::clone(&runtime));
    wire_pause(ui, Arc::clone(&runtime));
    wire_cancel(ui, Arc::clone(&runtime));
}

struct DownloadRuntime {
    ui: slint::Weak<AppWindow>,
    downloads: Arc<DownloadCoordinator>,
    storage: Arc<SqliteStorage>,
    installers: Arc<LocalModelInstallers>,
}

impl DownloadRuntime {
    fn start_ready(self: &Arc<Self>) {
        for (model, generation, control) in self.downloads.take_ready() {
            let Some(ui) = self.ui.upgrade() else {
                let _ = self.downloads.finish(model, generation);
                continue;
            };
            let progress = download_progress(&ui, model);
            set_download_state(&ui, model, LocalModelDownloadState::Downloading, progress);
            self.start_one(model, generation, control);
        }
    }

    fn start_one(
        self: &Arc<Self>,
        model: LocalModel,
        generation: u64,
        control: ModelInstallControl,
    ) {
        let installer = self.installers.for_model(model);
        let progress_ui = self.ui.clone();
        let progress_downloads = Arc::clone(&self.downloads);
        let runtime = Arc::clone(self);
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
                let completion = run_download(
                    model,
                    installer.as_ref(),
                    runtime.storage.as_ref(),
                    progress,
                    control,
                );
                if !runtime.downloads.finish(model, generation) {
                    return;
                }
                runtime.clear_persisted_queue_entry(model, &completion);
                let next_runtime = Arc::clone(&runtime);
                let _ = runtime.ui.upgrade_in_event_loop(move |ui| {
                    apply_completion(&ui, model, completion);
                    next_runtime.start_ready();
                });
            });
        if let Err(error) = spawn {
            let _ = self.downloads.finish(model, generation);
            if let Some(ui) = self.ui.upgrade() {
                apply_download_failure(
                    &ui,
                    model,
                    ModelInstallError::Filesystem(error.to_string()),
                    false,
                );
            }
            self.start_ready();
        }
    }

    fn discard_partial(self: &Arc<Self>, model: LocalModel) {
        let installer = self.installers.for_model(model);
        let runtime = Arc::clone(self);
        let spawn = std::thread::Builder::new()
            .name(model.thread_name("cancel-download"))
            .spawn(move || {
                let completion = match installer.discard_partial_download() {
                    Ok(()) => DownloadCompletion::Cancelled,
                    Err(error) => DownloadCompletion::Failed(error, false),
                };
                runtime.clear_persisted_queue_entry(model, &completion);
                let _ = runtime.ui.upgrade_in_event_loop(move |ui| {
                    apply_completion(&ui, model, completion);
                });
            });
        if let Err(error) = spawn
            && let Some(ui) = self.ui.upgrade()
        {
            apply_download_failure(
                &ui,
                model,
                ModelInstallError::Filesystem(error.to_string()),
                false,
            );
        }
    }

    fn clear_persisted_queue_entry(&self, model: LocalModel, completion: &DownloadCompletion) {
        if !completion.clears_persisted_queue_entry() {
            return;
        }
        if let Err(error) = self.storage.remove_model_download(model.id()) {
            tracing::warn!(
                event = "model.download_queue_remove_failed",
                model_id = model.id(),
                reason = %error
            );
        }
    }
}

fn restore_queue(ui: &AppWindow, storage: &SqliteStorage, installers: &LocalModelInstallers) {
    let queued = match storage.queued_model_downloads() {
        Ok(queued) => queued,
        Err(error) => {
            ui.set_local_model_operation_error(error.to_string().into());
            return;
        }
    };
    let plan = plan_restored_downloads(queued, |model| installers.for_model(model).is_installed());
    for stale_id in plan.stale_ids {
        if let Err(error) = storage.remove_model_download(&stale_id) {
            tracing::warn!(
                event = "model.download_queue_stale_remove_failed",
                model_id = stale_id,
                reason = %error
            );
        }
    }
    for model in plan.paused {
        let progress = download_progress(ui, model);
        set_download_state(ui, model, LocalModelDownloadState::Paused, progress);
    }
}

struct RestoredDownloadPlan {
    paused: Vec<LocalModel>,
    stale_ids: Vec<String>,
}

fn plan_restored_downloads(
    entries: Vec<QueuedModelDownload>,
    is_installed: impl Fn(LocalModel) -> bool,
) -> RestoredDownloadPlan {
    let mut paused = Vec::new();
    let mut stale_ids = Vec::new();
    for entry in entries {
        match LocalModel::from_id(&entry.model_id) {
            Some(model) if !is_installed(model) => paused.push(model),
            Some(_) | None => stale_ids.push(entry.model_id),
        }
    }
    RestoredDownloadPlan { paused, stale_ids }
}

fn wire_start(ui: &AppWindow, runtime: Arc<DownloadRuntime>) {
    ui.on_request_local_model_download(move |kind| {
        let Some((model, installer)) = installer_for(kind, &runtime.installers) else {
            return;
        };
        let Some(ui) = runtime.ui.upgrade() else {
            return;
        };
        if let Err(error) = installer.ensure_install_space() {
            apply_download_failure(&ui, model, error, false);
            return;
        }
        if let Err(error) = runtime.storage.enqueue_model_download(model.id()) {
            apply_download_failure(
                &ui,
                model,
                ModelInstallError::Activation(error.to_string()),
                false,
            );
            return;
        }
        runtime.downloads.enqueue(model);
        let progress = download_progress(&ui, model);
        set_download_state(&ui, model, LocalModelDownloadState::Queued, progress);
        ui.set_local_model_download_failure_visible(false);
        ui.set_local_model_download_space_failure(false);
        ui.set_local_model_download_error(SharedString::default());
        ui.set_local_model_operation_error(SharedString::default());
        runtime.start_ready();
    });
}

fn wire_pause(ui: &AppWindow, runtime: Arc<DownloadRuntime>) {
    ui.on_request_local_model_pause(move |kind| {
        let Some((model, _)) = installer_for(kind, &runtime.installers) else {
            return;
        };
        if !runtime
            .downloads
            .interrupt(model, ModelInstallInterruption::Paused)
        {
            return;
        }
        if let Some(ui) = runtime.ui.upgrade() {
            let progress = download_progress(&ui, model);
            set_download_state(&ui, model, LocalModelDownloadState::Paused, progress);
        }
    });
}

fn wire_cancel(ui: &AppWindow, runtime: Arc<DownloadRuntime>) {
    ui.on_request_local_model_cancel_download(move |kind| {
        let Some((model, _)) = installer_for(kind, &runtime.installers) else {
            return;
        };
        if runtime
            .downloads
            .interrupt(model, ModelInstallInterruption::Cancelled)
        {
            if let Some(ui) = runtime.ui.upgrade() {
                let progress = download_progress(&ui, model);
                set_download_state(&ui, model, LocalModelDownloadState::Cancelling, progress);
            }
            return;
        }
        let _ = runtime.downloads.remove_queued(model);
        let Some(ui) = runtime.ui.upgrade() else {
            return;
        };
        let progress = download_progress(&ui, model);
        set_download_state(&ui, model, LocalModelDownloadState::Cancelling, progress);
        runtime.discard_partial(model);
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
    fn coordinator_runs_at_most_two_downloads_in_fifo_order() {
        let coordinator = DownloadCoordinator::default();
        coordinator.enqueue(LocalModel::Whisper);
        coordinator.enqueue(LocalModel::Qwen3);
        coordinator.enqueue(LocalModel::Paraformer);
        let ready = coordinator.take_ready();
        assert_eq!(
            vec![LocalModel::Whisper, LocalModel::Qwen3],
            ready.iter().map(|(model, _, _)| *model).collect::<Vec<_>>()
        );
        let generation = ready[0].1;
        assert!(coordinator.take_ready().is_empty());
        assert!(coordinator.finish(LocalModel::Whisper, generation));
        assert_eq!(LocalModel::Paraformer, coordinator.take_ready()[0].0);
    }

    #[test]
    fn stale_completion_cannot_remove_a_newer_task() {
        let coordinator = DownloadCoordinator::default();
        coordinator.enqueue(LocalModel::Paraformer);
        let generation = coordinator.take_ready()[0].1;

        assert!(!coordinator.finish(LocalModel::Paraformer, generation + 1));
        assert!(coordinator.is_current(LocalModel::Paraformer, generation));
    }

    #[test]
    fn duplicate_queue_entries_are_ignored_and_can_be_cancelled() {
        let coordinator = DownloadCoordinator::default();
        coordinator.enqueue(LocalModel::SenseVoice);
        coordinator.enqueue(LocalModel::SenseVoice);
        coordinator.enqueue(LocalModel::Whisper);

        assert!(coordinator.remove_queued(LocalModel::SenseVoice));
        let ready = coordinator.take_ready();
        assert_eq!(1, ready.len());
        assert_eq!(LocalModel::Whisper, ready[0].0);
    }

    #[test]
    fn restored_downloads_wait_for_the_user_to_select_which_model_continues() {
        let plan = plan_restored_downloads(
            vec![
                QueuedModelDownload {
                    model_id: LocalModel::Qwen3.id().to_owned(),
                },
                QueuedModelDownload {
                    model_id: LocalModel::Paraformer.id().to_owned(),
                },
                QueuedModelDownload {
                    model_id: "unknown-model".to_owned(),
                },
                QueuedModelDownload {
                    model_id: LocalModel::SenseVoice.id().to_owned(),
                },
            ],
            |model| model == LocalModel::SenseVoice,
        );
        let coordinator = DownloadCoordinator::default();

        assert_eq!(vec![LocalModel::Qwen3, LocalModel::Paraformer], plan.paused);
        assert_eq!(
            vec!["unknown-model", LocalModel::SenseVoice.id()],
            plan.stale_ids
        );
        assert!(coordinator.take_ready().is_empty());

        coordinator.enqueue(plan.paused[1]);
        let ready = coordinator.take_ready();
        assert_eq!(1, ready.len());
        assert_eq!(LocalModel::Paraformer, ready[0].0);
    }

    #[test]
    fn persisted_queue_entry_is_cleared_only_after_terminal_cleanup() {
        let installed =
            DownloadCompletion::Installed(Ok((SharedString::default(), SharedString::default())));
        let paused = DownloadCompletion::Paused;
        let cancelled = DownloadCompletion::Cancelled;
        let failed = DownloadCompletion::Failed(
            ModelInstallError::Download("network unavailable".to_owned()),
            false,
        );

        assert!(installed.clears_persisted_queue_entry());
        assert!(!paused.clears_persisted_queue_entry());
        assert!(cancelled.clears_persisted_queue_entry());
        assert!(!failed.clears_persisted_queue_entry());
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
