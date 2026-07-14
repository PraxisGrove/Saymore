use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use template_infra::configure_overlay_window;

pub fn prepare(window: &slint::Window) -> Result<(), String> {
    configure(window)
}

pub fn present(window: &slint::Window) -> Result<(), String> {
    configure(window)?;
    window.show().map_err(|error| error.to_string())
}

fn configure(window: &slint::Window) -> Result<(), String> {
    let handle = appkit_view(window)?;
    unsafe { configure_overlay_window(handle).map_err(|error| error.to_string()) }
}

fn appkit_view(window: &slint::Window) -> Result<std::ptr::NonNull<std::ffi::c_void>, String> {
    let handle = window.window_handle();
    handle
        .window_handle()
        .map_err(|error| error.to_string())
        .and_then(|handle| match handle.as_raw() {
            RawWindowHandle::AppKit(handle) => Ok(handle.ns_view),
            _ => Err("the overlay does not have an AppKit window handle".to_owned()),
        })
}
