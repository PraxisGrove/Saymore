use std::{sync::Arc, time::Duration};

use slint::{ComponentHandle, Timer, TimerMode};
use template_app::{MicrophonePermissionProvider, TextDeliverer};

use crate::{
    AppWindow,
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

    let poll_ui = ui.as_weak();
    let timer = Timer::default();
    timer.start(
        TimerMode::Repeated,
        AUTHORIZATION_POLL_INTERVAL,
        move || {
            if let Some(ui) = poll_ui.upgrade() {
                update_authorizations(&ui, deliverer.authorization(), microphone.authorization());
            }
        },
    );
    AuthorizationPoll { _timer: timer }
}
