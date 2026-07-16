use std::time::Duration;

use slint::{ComponentHandle, Timer};
use template_infra::activate_application;

use crate::{
    overlay_window,
    ui::{AppPage, AppWindow, DictionaryAddedOverlay},
};

const NOTIFICATION_DURATION: Duration = Duration::from_secs(8);

pub fn wire(ui: &AppWindow, overlay: &DictionaryAddedOverlay) {
    let notification = overlay.as_weak();
    ui.on_show_dictionary_added(move |entry_id, entry| {
        let Some(overlay) = notification.upgrade() else {
            return;
        };
        let generation = overlay.get_notification_generation().saturating_add(1);
        overlay.set_notification_generation(generation);
        overlay.set_entry_id(entry_id);
        overlay.set_entry(entry);
        if let Err(error) = overlay_window::present(overlay.window()) {
            tracing::warn!(event = "dictionary.notification_show_failed", reason = %error);
            return;
        }

        let expiring_notification = overlay.as_weak();
        Timer::single_shot(NOTIFICATION_DURATION, move || {
            let Some(overlay) = expiring_notification.upgrade() else {
                return;
            };
            if overlay.get_notification_generation() == generation {
                let _ = overlay.hide();
            }
        });
    });

    let close_notification = overlay.as_weak();
    overlay.on_close_notification(move || {
        if let Some(overlay) = close_notification.upgrade() {
            let _ = overlay.hide();
        }
    });

    let undo_notification = overlay.as_weak();
    let undo_ui = ui.as_weak();
    overlay.on_undo_addition(move || {
        let Some(overlay) = undo_notification.upgrade() else {
            return;
        };
        let entry_id = overlay.get_entry_id();
        let _ = overlay.hide();
        if let Some(ui) = undo_ui.upgrade() {
            ui.invoke_delete_dictionary_word(entry_id);
        }
    });

    let view_notification = overlay.as_weak();
    let view_ui = ui.as_weak();
    overlay.on_view_dictionary(move || {
        let Some(ui) = view_ui.upgrade() else {
            return;
        };
        ui.set_current_page(AppPage::Dictionary);
        ui.invoke_refresh_dictionary();
        if let Err(error) = ui.show() {
            tracing::warn!(event = "dictionary.window_show_failed", reason = %error);
            return;
        }
        if let Err(error) = activate_application() {
            tracing::warn!(event = "dictionary.application_activate_failed", reason = %error);
        }
        if let Some(overlay) = view_notification.upgrade() {
            let _ = overlay.hide();
        }
    });
}
