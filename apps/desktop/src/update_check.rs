mod download;

use crate::ui::{AppWindow, Translations, UpdateDownloadPhase, UpdateDownloadUi};
use slint::{ComponentHandle, SharedString};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};
use template_app::{SoftwareUpdater, UpdateCancellation, UpdatePackage};
use template_infra::{GithubSoftwareUpdater, UpdatePlatform};

struct UpdateState {
    updater: Arc<dyn SoftwareUpdater>,
    busy: AtomicBool,
    package: Mutex<Option<UpdatePackage>>,
    installer: Mutex<Option<PathBuf>>,
    cancellation: Mutex<UpdateCancellation>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CheckPresentation {
    Settings,
    StartupNotification,
}

pub fn wire(ui: &AppWindow, check_on_startup: bool, directory: PathBuf) {
    let updater = match GithubSoftwareUpdater::new(directory, UpdatePlatform::current()) {
        Ok(updater) => updater,
        Err(_) => {
            ui.set_update_status("failed".into());
            ui.set_update_detail(ui.global::<Translations>().get_update_service_unavailable());
            return;
        }
    };
    let state = Arc::new(UpdateState {
        updater: Arc::new(updater),
        busy: AtomicBool::new(false),
        package: Mutex::new(None),
        installer: Mutex::new(None),
        cancellation: Mutex::new(UpdateCancellation::default()),
    });
    let check_ui = ui.as_weak();
    let check_state = state.clone();
    ui.on_check_for_updates(move || {
        start_check(
            check_ui.clone(),
            check_state.clone(),
            CheckPresentation::Settings,
        )
    });
    let download_ui = ui.as_weak();
    let download_state = state.clone();
    ui.on_download_update(move || download::start(download_ui.clone(), download_state.clone()));
    let cancel_state = state.clone();
    ui.global::<UpdateDownloadUi>().on_cancel(move || {
        if let Ok(control) = cancel_state.cancellation.lock() {
            control.cancel();
        }
    });
    let dismiss_ui = ui.as_weak();
    ui.on_dismiss_update_notification(move || {
        if let Some(window) = dismiss_ui.upgrade() {
            window.set_update_notification_visible(false);
        }
    });
    if check_on_startup {
        start_check(ui.as_weak(), state, CheckPresentation::StartupNotification);
    }
}

fn start_check(
    ui: slint::Weak<AppWindow>,
    state: Arc<UpdateState>,
    presentation: CheckPresentation,
) {
    if state.busy.swap(true, Ordering::AcqRel) {
        return;
    }
    if let Some(window) = ui.upgrade() {
        window.set_update_status("checking".into());
        window.set_update_detail(SharedString::new());
    }
    let failure_ui = ui.clone();
    let worker_state = state.clone();
    let spawn = thread::Builder::new()
        .name("saymore-check-updates".into())
        .spawn(move || {
            let result = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|_| template_app::UpdateError::Network)
                .and_then(|runtime| {
                    runtime.block_on(worker_state.updater.latest(env!("CARGO_PKG_VERSION")))
                });
            let _ = ui.upgrade_in_event_loop(move |window| {
                let available = matches!(&result, Ok(Some(_)));
                if let Ok(mut package) = worker_state.package.lock() {
                    *package = result.as_ref().ok().and_then(Clone::clone);
                }
                if let Ok(mut installer) = worker_state.installer.lock() {
                    *installer = None;
                }
                window
                    .global::<UpdateDownloadUi>()
                    .set_phase(UpdateDownloadPhase::Idle);
                match result {
                    Ok(Some(package)) => {
                        window.set_update_status("available".into());
                        window.set_update_version(package.version.into());
                        window.set_update_detail(SharedString::new());
                    }
                    Ok(None) => {
                        window.set_update_status("latest".into());
                        window.set_update_version(SharedString::new());
                        window.set_update_detail(SharedString::new());
                    }
                    Err(_) => {
                        window.set_update_status("failed".into());
                        window.set_update_version(SharedString::new());
                        window.set_update_detail(
                            window
                                .global::<Translations>()
                                .get_update_service_unavailable(),
                        );
                    }
                }
                if presentation == CheckPresentation::StartupNotification {
                    window.set_update_notification_visible(available);
                }
                worker_state.busy.store(false, Ordering::Release);
            });
        });
    if spawn.is_err() {
        state.busy.store(false, Ordering::Release);
        if let Some(window) = failure_ui.upgrade() {
            window.set_update_status("failed".into());
            window.set_update_detail(
                window
                    .global::<Translations>()
                    .get_update_check_start_failed(),
            );
        }
    }
}
