#[cfg(target_os = "macos")]
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use slint::ComponentHandle;
#[cfg(target_os = "macos")]
use slint::Timer;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use slint::winit_030::{EventResult, WinitWindowAccessor, winit::event::WindowEvent};
#[cfg(target_os = "macos")]
use std::time::Duration;
use template_app::LocalSettings;
use template_infra::AppEnvironment;
#[cfg(target_os = "macos")]
use template_infra::configure_main_window;

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
    ui.set_show_dock_setting(cfg!(target_os = "macos"));
    ui.set_show_accessibility_setting(cfg!(target_os = "macos"));
    #[cfg(target_os = "windows")]
    crate::windows_window::integrate(ui);
    configure_close_behavior(ui);
    Ok(context)
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn configure_close_behavior(ui: &AppWindow) {
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

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn configure_close_behavior(_ui: &AppWindow) {}

#[cfg(target_os = "macos")]
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

#[cfg(target_os = "windows")]
pub fn schedule_titlebar_integration(ui: &AppWindow) {
    crate::windows_window::refresh(ui);
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn schedule_titlebar_integration(_ui: &AppWindow) {}

#[cfg(target_os = "macos")]
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
