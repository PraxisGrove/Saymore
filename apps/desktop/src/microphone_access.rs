use slint::ComponentHandle;
use template_app::{MicrophoneAuthorization, MicrophonePermissionProvider};
use template_infra::{MacOsMicrophonePermission, open_microphone_privacy_settings};

use crate::{
    overlay_window,
    ui::{AppWindow, MicrophoneIntroOverlay, MicrophonePermissionOverlay},
    ui_status::update_microphone_authorization,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShortcutPermissionAction {
    Record,
    ExplainFirstUse,
    OpenSettings,
}

#[derive(Clone)]
pub struct MicrophoneAccess {
    intro: slint::Weak<MicrophoneIntroOverlay>,
    permission: slint::Weak<MicrophonePermissionOverlay>,
    provider: MacOsMicrophonePermission,
}

pub fn wire(
    ui: &AppWindow,
    intro: &MicrophoneIntroOverlay,
    permission: &MicrophonePermissionOverlay,
    provider: MacOsMicrophonePermission,
) -> MicrophoneAccess {
    let access = MicrophoneAccess {
        intro: intro.as_weak(),
        permission: permission.as_weak(),
        provider,
    };

    let dismiss_intro = intro.as_weak();
    intro.on_dismiss(move || hide(&dismiss_intro));

    let continue_intro = intro.as_weak();
    let continue_ui = ui.as_weak();
    intro.on_continue_requested(move || {
        hide(&continue_intro);
        if let Some(ui) = continue_ui.upgrade() {
            update_microphone_authorization(&ui, provider.request_authorization());
        }
    });

    let dismiss_permission = permission.as_weak();
    permission.on_dismiss(move || hide(&dismiss_permission));

    let settings_permission = permission.as_weak();
    permission.on_open_settings(move || {
        hide(&settings_permission);
        if let Err(error) = open_microphone_privacy_settings() {
            tracing::warn!(event = "microphone.settings_open_failed", reason = %error);
        }
    });

    let settings_ui = ui.as_weak();
    ui.on_request_microphone_authorization(move || {
        if let Some(ui) = settings_ui.upgrade() {
            update_microphone_authorization(&ui, provider.request_authorization());
        }
    });

    access
}

impl MicrophoneAccess {
    pub fn allows_recording(&self) -> bool {
        match shortcut_permission_action(self.provider.authorization()) {
            ShortcutPermissionAction::Record => true,
            ShortcutPermissionAction::ExplainFirstUse => {
                show_intro(self.intro.clone());
                false
            }
            ShortcutPermissionAction::OpenSettings => {
                show_permission(self.permission.clone());
                false
            }
        }
    }
}

fn shortcut_permission_action(authorization: MicrophoneAuthorization) -> ShortcutPermissionAction {
    match authorization {
        MicrophoneAuthorization::NotDetermined => ShortcutPermissionAction::ExplainFirstUse,
        MicrophoneAuthorization::Granted => ShortcutPermissionAction::Record,
        MicrophoneAuthorization::Denied | MicrophoneAuthorization::Restricted => {
            ShortcutPermissionAction::OpenSettings
        }
    }
}

fn show_intro(overlay: slint::Weak<MicrophoneIntroOverlay>) {
    let _ = overlay.upgrade_in_event_loop(|overlay| {
        if let Err(error) = overlay_window::present(overlay.window()) {
            tracing::warn!(event = "microphone.intro_present_failed", reason = %error);
        }
    });
}

fn show_permission(overlay: slint::Weak<MicrophonePermissionOverlay>) {
    let _ = overlay.upgrade_in_event_loop(|overlay| {
        if let Err(error) = overlay_window::present(overlay.window()) {
            tracing::warn!(event = "microphone.permission_present_failed", reason = %error);
        }
    });
}

fn hide<T: ComponentHandle>(overlay: &slint::Weak<T>) {
    if let Some(overlay) = overlay.upgrade() {
        let _ = overlay.hide();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_microphone_authorization_to_shortcut_behavior() {
        assert_eq!(
            [
                ShortcutPermissionAction::ExplainFirstUse,
                ShortcutPermissionAction::Record,
                ShortcutPermissionAction::OpenSettings,
                ShortcutPermissionAction::OpenSettings,
            ],
            [
                MicrophoneAuthorization::NotDetermined,
                MicrophoneAuthorization::Granted,
                MicrophoneAuthorization::Denied,
                MicrophoneAuthorization::Restricted,
            ]
            .map(shortcut_permission_action)
        );
    }
}
