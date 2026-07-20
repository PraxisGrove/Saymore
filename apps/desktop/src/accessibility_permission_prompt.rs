use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use slint::ComponentHandle;
use template_infra::open_accessibility_privacy_settings;

use crate::{AccessibilityPermissionOverlay, overlay_window};

#[derive(Clone)]
pub(crate) struct AccessibilityPermissionPrompt {
    permission: slint::Weak<AccessibilityPermissionOverlay>,
    initial_reminder: InitialPermissionReminder,
}

#[derive(Clone, Default)]
pub(crate) struct InitialPermissionReminder {
    presented: Arc<AtomicBool>,
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
        initial_reminder: InitialPermissionReminder::default(),
    }
}

impl AccessibilityPermissionPrompt {
    pub(crate) fn show_required(&self) {
        present(self.permission.clone());
    }

    pub(crate) fn show_initial_if_required(
        &self,
        onboarding_active: bool,
        accessibility_authorized: bool,
    ) {
        let permission = self.permission.clone();
        handle_initial_required(
            &self.initial_reminder,
            onboarding_active,
            accessibility_authorized,
            move || present(permission),
        );
    }
}

pub(crate) fn handle_initial_required(
    reminder: &InitialPermissionReminder,
    onboarding_active: bool,
    accessibility_authorized: bool,
    show_prompt: impl FnOnce(),
) {
    if onboarding_active || accessibility_authorized {
        return;
    }
    if reminder
        .presented
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        show_prompt();
    }
}

pub(crate) fn handle_required_shortcut(
    onboarding_active: &(dyn Fn() -> bool + Send + Sync),
    show_prompt: impl FnOnce(),
) {
    if !onboarding_active() {
        show_prompt();
    }
}

pub(crate) fn open_settings() {
    if let Err(error) = open_accessibility_privacy_settings() {
        tracing::warn!(event = "accessibility.settings_open_failed", reason = %error);
    }
}

fn present(permission: slint::Weak<AccessibilityPermissionOverlay>) {
    let _ = permission.upgrade_in_event_loop(|overlay| {
        if let Err(error) = overlay_window::present(overlay.window()) {
            tracing::warn!(event = "accessibility.permission_present_failed", reason = %error);
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
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::{InitialPermissionReminder, handle_initial_required, handle_required_shortcut};

    #[test]
    fn required_shortcut_prompt_is_suppressed_while_onboarding_owns_local_input() {
        let prompts = AtomicUsize::new(0);

        handle_required_shortcut(&|| true, || {
            prompts.fetch_add(1, Ordering::Relaxed);
        });
        assert_eq!(0, prompts.load(Ordering::Relaxed));

        handle_required_shortcut(&|| false, || {
            prompts.fetch_add(1, Ordering::Relaxed);
        });
        assert_eq!(1, prompts.load(Ordering::Relaxed));
    }

    #[test]
    fn initial_prompt_waits_for_onboarding_and_is_shown_only_once() {
        let reminder = InitialPermissionReminder::default();
        let prompts = AtomicUsize::new(0);

        handle_initial_required(&reminder, true, false, || {
            prompts.fetch_add(1, Ordering::Relaxed);
        });
        handle_initial_required(&reminder, false, false, || {
            prompts.fetch_add(1, Ordering::Relaxed);
        });
        handle_initial_required(&reminder, false, false, || {
            prompts.fetch_add(1, Ordering::Relaxed);
        });

        assert_eq!(1, prompts.load(Ordering::Relaxed));
    }

    #[test]
    fn initial_prompt_is_not_shown_when_accessibility_is_authorized() {
        let reminder = InitialPermissionReminder::default();
        let prompts = AtomicUsize::new(0);

        handle_initial_required(&reminder, false, true, || {
            prompts.fetch_add(1, Ordering::Relaxed);
        });

        assert_eq!(0, prompts.load(Ordering::Relaxed));
    }
}
