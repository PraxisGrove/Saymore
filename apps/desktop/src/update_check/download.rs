use super::UpdateState;
use crate::{
    platform_open,
    ui::{AppPage, AppWindow, SettingsSection, UpdateDownloadPhase, UpdateDownloadUi},
};
use slint::ComponentHandle;
use std::{
    sync::{Arc, Mutex, atomic::Ordering},
    thread,
    time::{Duration, Instant},
};
use template_app::{UpdateCancellation, UpdateError, UpdateProgress};

pub(super) fn start(ui: slint::Weak<AppWindow>, state: Arc<UpdateState>) {
    if state.busy.swap(true, Ordering::AcqRel) {
        return;
    }
    let installer = state.installer.lock().ok().and_then(|value| value.clone());
    if let Some(path) = installer {
        let result = platform_open::open(path);
        if let Some(window) = ui.upgrade() {
            window
                .global::<UpdateDownloadUi>()
                .set_error_kind(if result.is_ok() { "" } else { "open" }.into());
        }
        state.busy.store(false, Ordering::Release);
        return;
    }
    let Some(package) = state.package.lock().ok().and_then(|value| value.clone()) else {
        state.busy.store(false, Ordering::Release);
        return;
    };
    let control = UpdateCancellation::default();
    let Ok(mut stored) = state.cancellation.lock() else {
        state.busy.store(false, Ordering::Release);
        return;
    };
    *stored = control.clone();
    drop(stored);
    if let Some(window) = ui.upgrade() {
        window.set_update_notification_visible(false);
        window.set_current_page(AppPage::Settings);
        window.set_settings_section(SettingsSection::AboutUpdates);
        let download = window.global::<UpdateDownloadUi>();
        download.set_phase(UpdateDownloadPhase::Downloading);
        download.set_fraction(0.0);
        download.set_bytes_label("0 MB".into());
        download.set_error_kind("".into());
    }
    let progress = progress_callback(ui.clone());
    let worker_state = state.clone();
    let failure_ui = ui.clone();
    let spawn = thread::Builder::new()
        .name("saymore-download-update".into())
        .spawn(move || {
            let result = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|_| UpdateError::Network)
                .and_then(|runtime| {
                    runtime.block_on(worker_state.updater.download(&package, control, progress))
                });
            let _ = ui.upgrade_in_event_loop(move |window| {
                let download = window.global::<UpdateDownloadUi>();
                match result {
                    Ok(path) => {
                        if let Ok(mut installer) = worker_state.installer.lock() {
                            *installer = Some(path);
                            download.set_phase(UpdateDownloadPhase::Ready);
                            download.set_fraction(1.0);
                        } else {
                            download.set_phase(UpdateDownloadPhase::Failed);
                            download.set_error_kind("storage".into());
                        }
                    }
                    Err(UpdateError::Cancelled) => {
                        download.set_phase(UpdateDownloadPhase::Cancelled)
                    }
                    Err(error) => {
                        tracing::warn!(event = "update.download_failed");
                        download.set_phase(UpdateDownloadPhase::Failed);
                        download.set_error_kind(
                            match error {
                                UpdateError::Integrity | UpdateError::Metadata => "integrity",
                                UpdateError::Storage => "storage",
                                _ => "network",
                            }
                            .into(),
                        );
                    }
                }
                worker_state.busy.store(false, Ordering::Release);
            });
        });
    if spawn.is_err() {
        state.busy.store(false, Ordering::Release);
        if let Some(window) = failure_ui.upgrade() {
            window
                .global::<UpdateDownloadUi>()
                .set_phase(UpdateDownloadPhase::Failed);
            window
                .global::<UpdateDownloadUi>()
                .set_error_kind("start".into());
        }
    }
}

fn progress_callback(ui: slint::Weak<AppWindow>) -> Arc<dyn Fn(UpdateProgress) + Send + Sync> {
    let last = Mutex::new(Instant::now());
    Arc::new(move |progress| {
        if let UpdateProgress::Downloading { received, total } = progress {
            let Ok(mut last) = last.lock() else {
                return;
            };
            if received != total && last.elapsed() < Duration::from_millis(100) {
                return;
            }
            *last = Instant::now();
        }
        let _ = ui.upgrade_in_event_loop(move |window| {
            let download = window.global::<UpdateDownloadUi>();
            match progress {
                UpdateProgress::Downloading { received, total } => {
                    download.set_fraction(if total == 0 {
                        0.0
                    } else {
                        (received as f64 / total as f64) as f32
                    });
                    download.set_bytes_label(
                        format!(
                            "{:.1} / {:.1} MB",
                            received as f64 / 1_000_000.0,
                            total as f64 / 1_000_000.0
                        )
                        .into(),
                    );
                }
                UpdateProgress::Verifying => download.set_phase(UpdateDownloadPhase::Verifying),
            }
        });
    })
}
