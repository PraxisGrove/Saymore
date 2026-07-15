use std::{
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use serde::Deserialize;
use slint::{ComponentHandle, SharedString};

use crate::ui::{AppWindow, Translations};

const RELEASES_URL: &str = "https://api.github.com/repos/PraxisGrove/Saymore/releases/latest";
const RELEASES_PREFIX: &str = "https://github.com/PraxisGrove/Saymore/releases/";

#[derive(Default)]
struct UpdateState {
    checking: AtomicBool,
    download_url: Mutex<Option<String>>,
}

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    prerelease: bool,
}

pub fn wire(ui: &AppWindow) {
    let state = Arc::new(UpdateState::default());
    let check_ui = ui.as_weak();
    let check_state = Arc::clone(&state);
    ui.on_check_for_updates(move || start_check(check_ui.clone(), Arc::clone(&check_state)));

    let download_ui = ui.as_weak();
    ui.on_download_update(move || {
        let url = state
            .download_url
            .lock()
            .ok()
            .and_then(|guard| guard.clone());
        open_download_page(download_ui.clone(), url);
    });
}

fn start_check(ui: slint::Weak<AppWindow>, state: Arc<UpdateState>) {
    if state
        .checking
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    if let Some(window) = ui.upgrade() {
        window.set_update_status(SharedString::from("checking"));
        window.set_update_detail(SharedString::new());
    }

    let failure_ui = ui.clone();
    let worker_state = Arc::clone(&state);
    if thread::Builder::new()
        .name("saymore-check-updates".to_owned())
        .spawn(move || {
            let result = latest_release();
            worker_state.checking.store(false, Ordering::Release);
            let _ = ui.upgrade_in_event_loop(move |window| match result {
                Ok(Some(release)) => {
                    if let Ok(mut download_url) = worker_state.download_url.lock() {
                        *download_url = Some(release.html_url);
                    }
                    window.set_update_status(SharedString::from("available"));
                    window.set_update_version(SharedString::from(release.tag_name));
                    window.set_update_detail(SharedString::new());
                }
                Ok(None) => {
                    if let Ok(mut download_url) = worker_state.download_url.lock() {
                        *download_url = None;
                    }
                    window.set_update_status(SharedString::from("latest"));
                    window.set_update_version(SharedString::new());
                    window.set_update_detail(SharedString::new());
                }
                Err(()) => {
                    window.set_update_status(SharedString::from("failed"));
                    window.set_update_version(SharedString::new());
                    window.set_update_detail(
                        window
                            .global::<Translations>()
                            .get_update_service_unavailable(),
                    );
                }
            });
        })
        .is_err()
    {
        state.checking.store(false, Ordering::Release);
        if let Some(window) = failure_ui.upgrade() {
            window.set_update_status(SharedString::from("failed"));
            window.set_update_detail(
                window
                    .global::<Translations>()
                    .get_update_check_start_failed(),
            );
        }
    }
}

fn open_download_page(ui: slint::Weak<AppWindow>, url: Option<String>) {
    let Some(url) = url.filter(|value| value.starts_with(RELEASES_PREFIX)) else {
        if let Some(window) = ui.upgrade() {
            window.set_update_status(SharedString::from("failed"));
            window.set_update_detail(
                window
                    .global::<Translations>()
                    .get_update_download_url_unavailable(),
            );
        }
        return;
    };

    let _ = thread::Builder::new()
        .name("saymore-open-update-download".to_owned())
        .spawn(move || {
            let result = Command::new("/usr/bin/open").arg(url).status();
            if let Err(error) = result {
                tracing::warn!(event = "update.download_page_open_failed", reason = %error);
                let _ = ui.upgrade_in_event_loop(move |window| {
                    window.set_update_status(SharedString::from("failed"));
                    window.set_update_detail(
                        window
                            .global::<Translations>()
                            .get_update_open_download_failed(),
                    );
                });
            }
        });
}

fn latest_release() -> Result<Option<Release>, ()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| ())?;
    runtime.block_on(async {
        let client = reqwest::Client::builder()
            .user_agent("Saymore update checker")
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|_| ())?;
        let release = client
            .get(RELEASES_URL)
            .send()
            .await
            .map_err(|_| ())?
            .error_for_status()
            .map_err(|_| ())?
            .json::<Release>()
            .await
            .map_err(|_| ())?;
        let has_update = !release.prerelease
            && release.html_url.starts_with(RELEASES_PREFIX)
            && is_newer_version(&release.tag_name, env!("CARGO_PKG_VERSION"));
        Ok(has_update.then_some(release))
    })
}

fn is_newer_version(remote: &str, current: &str) -> bool {
    match (version_numbers(remote), version_numbers(current)) {
        (Some(remote), Some(current)) => remote > current,
        _ => false,
    }
}

fn version_numbers(value: &str) -> Option<[u64; 3]> {
    let value = value.trim().trim_start_matches('v');
    let core = value.split(['-', '+']).next()?;
    let mut numbers = [0_u64; 3];
    for (index, part) in core.split('.').enumerate() {
        if index >= numbers.len()
            || part.is_empty()
            || !part.bytes().all(|byte| byte.is_ascii_digit())
        {
            return None;
        }
        numbers[index] = part.parse().ok()?;
    }
    Some(numbers)
}

#[cfg(test)]
mod tests {
    use super::{is_newer_version, version_numbers};

    #[test]
    fn compares_release_versions_without_a_semver_dependency() {
        assert!(is_newer_version("v0.2.0", "0.1.0"));
        assert!(!is_newer_version("v0.1.0", "0.1.0"));
        assert!(!is_newer_version("v0.1.0-beta.1", "0.1.0"));
        assert!(!is_newer_version("bad", "0.1.0"));
    }

    #[test]
    fn accepts_missing_minor_or_patch_numbers() {
        assert_eq!(Some([2, 0, 0]), version_numbers("v2"));
        assert_eq!(Some([2, 3, 0]), version_numbers("2.3"));
    }
}
