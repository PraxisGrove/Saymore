use std::{sync::Arc, time::Duration};

use slint::{ComponentHandle, Timer, TimerMode};
use template_app::{MicrophonePermissionProvider, TextDeliverer};
#[cfg(target_os = "windows")]
use template_infra::open_windows_microphone_privacy_settings;
#[cfg(target_os = "macos")]
use template_infra::{open_accessibility_privacy_settings, open_microphone_privacy_settings};

use crate::{
    AppWindow,
    permission_actions::{PermissionAction, microphone_permission_action},
    ui_status::{update_accessibility_authorization, update_authorizations},
};

const AUTHORIZATION_POLL_INTERVAL: Duration = Duration::from_secs(1);

pub(crate) struct AuthorizationPoll {
    _timer: Timer,
}

pub(crate) fn wire(
    ui: &AppWindow,
    deliverer: Arc<dyn TextDeliverer>,
    microphone: Arc<dyn MicrophonePermissionProvider>,
) -> AuthorizationPoll {
    let request_accessibility_ui = ui.as_weak();
    let request_deliverer = Arc::clone(&deliverer);
    ui.on_request_authorization(move || {
        if let Some(ui) = request_accessibility_ui.upgrade() {
            update_accessibility_authorization(&ui, request_deliverer.request_authorization());
        }
    });

    let open_microphone_ui = ui.as_weak();
    let open_microphone_deliverer = Arc::clone(&deliverer);
    let open_microphone = Arc::clone(&microphone);
    ui.on_open_microphone_settings(move || {
        match microphone_permission_action(open_microphone.authorization()) {
            PermissionAction::Request => {
                open_microphone.request_authorization();
            }
            PermissionAction::OpenSettings => {
                if let Err(error) = open_platform_microphone_settings() {
                    tracing::warn!(event = "microphone.settings_open_failed", reason = %error);
                }
            }
        }
        refresh(
            &open_microphone_ui,
            &*open_microphone_deliverer,
            &*open_microphone,
        );
    });

    let open_accessibility_ui = ui.as_weak();
    let open_accessibility_deliverer = Arc::clone(&deliverer);
    let open_accessibility_microphone = Arc::clone(&microphone);
    ui.on_open_accessibility_settings(move || {
        open_accessibility_settings();
        refresh(
            &open_accessibility_ui,
            &*open_accessibility_deliverer,
            &*open_accessibility_microphone,
        );
    });

    let manual_refresh_ui = ui.as_weak();
    let manual_refresh_deliverer = Arc::clone(&deliverer);
    let manual_refresh_microphone = Arc::clone(&microphone);
    ui.on_refresh_authorizations(move || {
        refresh(
            &manual_refresh_ui,
            &*manual_refresh_deliverer,
            &*manual_refresh_microphone,
        );
    });

    let poll_ui = ui.as_weak();
    let timer = Timer::default();
    timer.start(
        TimerMode::Repeated,
        AUTHORIZATION_POLL_INTERVAL,
        move || {
            refresh(&poll_ui, &*deliverer, &*microphone);
        },
    );
    ui.invoke_refresh_authorizations();
    AuthorizationPoll { _timer: timer }
}

fn refresh(
    ui: &slint::Weak<AppWindow>,
    deliverer: &dyn TextDeliverer,
    microphone: &dyn MicrophonePermissionProvider,
) {
    if let Some(ui) = ui.upgrade() {
        update_authorizations(&ui, deliverer.authorization(), microphone.authorization());
    }
}

fn open_accessibility_settings() {
    if let Err(error) = open_platform_accessibility_settings() {
        tracing::warn!(event = "accessibility.settings_open_failed", reason = %error);
    }
}

#[cfg(target_os = "macos")]
fn open_platform_microphone_settings() -> Result<(), String> {
    open_microphone_privacy_settings().map_err(|error| error.to_string())
}

#[cfg(target_os = "windows")]
fn open_platform_microphone_settings() -> Result<(), String> {
    open_windows_microphone_privacy_settings().map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
fn open_platform_accessibility_settings() -> Result<(), String> {
    open_accessibility_privacy_settings().map_err(|error| error.to_string())
}

#[cfg(not(target_os = "macos"))]
fn open_platform_accessibility_settings() -> Result<(), String> {
    Err("accessibility settings integration is unavailable on this platform".to_owned())
}
