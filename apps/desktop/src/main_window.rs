use std::time::Duration;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::winit_030::{EventResult, WinitWindowAccessor, winit::event::WindowEvent};
use slint::{ComponentHandle, Timer};
use template_app::LocalSettings;
use template_infra::{AppEnvironment, configure_main_window};

use crate::{
    i18n::{self, LanguageContext},
    ui::AppWindow,
};

pub fn initialize(
    ui: &AppWindow,
    settings: &LocalSettings,
    environment: AppEnvironment,
) -> Result<LanguageContext, slint::SelectBundledTranslationError> {
    let context = i18n::initialize(ui, settings.ui_language)?;
    ui.set_automatic_update_checks(settings.automatic_update_checks);
    ui.set_feedback_sounds_enabled(settings.feedback_sounds_enabled);
    ui.set_development_environment(environment == AppEnvironment::Development);
    keep_running_after_main_window_close(ui);
    Ok(context)
}

fn keep_running_after_main_window_close(ui: &AppWindow) {
    ui.window().on_winit_window_event(|window, event| {
        if !matches!(event, WindowEvent::CloseRequested) {
            return EventResult::Propagate;
        }

        if let Err(error) = window.hide() {
            tracing::warn!(event = "main_window.hide_failed", reason = %error);
        }
        EventResult::PreventDefault
    });
}

pub fn schedule_titlebar_integration(ui: &AppWindow) {
    let initial_ui = ui.as_weak();
    Timer::single_shot(Duration::from_millis(100), move || {
        let Some(ui) = initial_ui.upgrade() else {
            return;
        };
        if integrate_titlebar(&ui).is_ok() {
            return;
        }

        let retry_ui = ui.as_weak();
        Timer::single_shot(Duration::from_millis(400), move || {
            if let Some(ui) = retry_ui.upgrade()
                && let Err(error) = integrate_titlebar(&ui)
            {
                eprintln!("failed to integrate the main window titlebar: {error}");
            }
        });
    });
}

fn integrate_titlebar(ui: &AppWindow) -> Result<(), String> {
    let handle = ui.window().window_handle();
    handle
        .window_handle()
        .map_err(|error| error.to_string())
        .and_then(|handle| match handle.as_raw() {
            RawWindowHandle::AppKit(handle) => unsafe {
                configure_main_window(handle.ns_view).map_err(|error| error.to_string())
            },
            _ => Err("the main window does not have an AppKit window handle".to_owned()),
        })
}
