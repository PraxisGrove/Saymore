use slint::ComponentHandle;
use template_infra::open_accessibility_privacy_settings;

use crate::{AccessibilityPermissionOverlay, overlay_window};

#[derive(Clone)]
pub(crate) struct AccessibilityPermissionPrompt {
    permission: slint::Weak<AccessibilityPermissionOverlay>,
}

pub(crate) fn wire(permission: &AccessibilityPermissionOverlay) -> AccessibilityPermissionPrompt {
    let dismiss_permission = permission.as_weak();
    permission.on_dismiss(move || hide(&dismiss_permission));

    let settings_permission = permission.as_weak();
    permission.on_open_settings(move || {
        hide(&settings_permission);
        open_settings();
    });

    AccessibilityPermissionPrompt {
        permission: permission.as_weak(),
    }
}

impl AccessibilityPermissionPrompt {
    pub(crate) fn show_required(&self) {
        let permission = self.permission.clone();
        let _ = permission.upgrade_in_event_loop(|overlay| {
            if let Err(error) = overlay_window::present(overlay.window()) {
                tracing::warn!(event = "accessibility.permission_present_failed", reason = %error);
            }
        });
    }
}

pub(crate) fn open_settings() {
    if let Err(error) = open_accessibility_privacy_settings() {
        tracing::warn!(event = "accessibility.settings_open_failed", reason = %error);
    }
}

fn hide<T: ComponentHandle>(overlay: &slint::Weak<T>) {
    if let Some(overlay) = overlay.upgrade() {
        let _ = overlay.hide();
    }
}
