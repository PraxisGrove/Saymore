use std::time::Duration;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::{ComponentHandle, Timer};
use template_infra::configure_main_window;

use crate::ui::AppWindow;

pub fn schedule_titlebar_integration(ui: &AppWindow) {
    let initial_ui = ui.as_weak();
    Timer::single_shot(Duration::from_millis(100), move || {
        if let Some(ui) = initial_ui.upgrade() {
            integrate_titlebar(&ui);
        }
    });

    let settled_ui = ui.as_weak();
    Timer::single_shot(Duration::from_millis(500), move || {
        if let Some(ui) = settled_ui.upgrade() {
            integrate_titlebar(&ui);
        }
    });
}

fn integrate_titlebar(ui: &AppWindow) {
    let handle = ui.window().window_handle();
    let result = handle
        .window_handle()
        .map_err(|error| error.to_string())
        .and_then(|handle| match handle.as_raw() {
            RawWindowHandle::AppKit(handle) => unsafe {
                configure_main_window(handle.ns_view).map_err(|error| error.to_string())
            },
            _ => Err("the main window does not have an AppKit window handle".to_owned()),
        });
    if let Err(error) = result {
        eprintln!("failed to integrate the main window titlebar: {error}");
    }
}
