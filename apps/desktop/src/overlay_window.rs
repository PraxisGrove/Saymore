use raw_window_handle::{HasWindowHandle, RawWindowHandle};
#[cfg(target_os = "macos")]
use template_infra::configure_overlay_window;
#[cfg(target_os = "windows")]
use template_infra::configure_windows_overlay_window;

pub fn prepare(window: &slint::Window) -> Result<(), String> {
    configure(window)
}

pub fn present(window: &slint::Window) -> Result<(), String> {
    configure(window)?;
    window.show().map_err(|error| error.to_string())?;
    #[cfg(target_os = "windows")]
    configure(window)?;
    Ok(())
}

fn configure(window: &slint::Window) -> Result<(), String> {
    let handle = window.window_handle();
    let handle = handle.window_handle().map_err(|error| error.to_string())?;
    configure_platform_overlay(handle.as_raw())
}

#[cfg(target_os = "macos")]
fn configure_platform_overlay(handle: RawWindowHandle) -> Result<(), String> {
    match handle {
        RawWindowHandle::AppKit(handle) => unsafe {
            configure_overlay_window(handle.ns_view).map_err(|error| error.to_string())
        },
        _ => Err("the overlay does not have an AppKit window handle".to_owned()),
    }
}

#[cfg(target_os = "windows")]
fn configure_platform_overlay(handle: RawWindowHandle) -> Result<(), String> {
    match handle {
        RawWindowHandle::Win32(handle) => {
            configure_windows_overlay_window(handle.hwnd.get()).map_err(|error| error.to_string())
        }
        _ => Err("the overlay does not have a Win32 window handle".to_owned()),
    }
}
