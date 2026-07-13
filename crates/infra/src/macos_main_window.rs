use std::{ffi::c_void, ptr::NonNull};

use objc2::{MainThreadMarker, rc::Retained};
use objc2_app_kit::{NSTitlebarSeparatorStyle, NSView, NSWindowStyleMask, NSWindowTitleVisibility};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MacOsMainWindowError {
    #[error("the main window must be configured on the macOS main thread")]
    NotMainThread,
    #[error("the Slint native view is no longer available")]
    MissingView,
    #[error("the Slint native view is not attached to a window")]
    MissingWindow,
    #[error("macOS did not apply the integrated titlebar configuration")]
    ConfigurationRejected,
}

/// Extends the Slint content behind the native titlebar while retaining the
/// standard macOS traffic-light controls.
///
/// # Safety
///
/// `ns_view` must point to a live `NSView` from a raw AppKit window handle and
/// remain valid for the duration of this call.
pub unsafe fn configure_main_window(ns_view: NonNull<c_void>) -> Result<(), MacOsMainWindowError> {
    let _mtm = MainThreadMarker::new().ok_or(MacOsMainWindowError::NotMainThread)?;
    let view = unsafe { Retained::<NSView>::retain(ns_view.as_ptr().cast()) }
        .ok_or(MacOsMainWindowError::MissingView)?;
    let window = view.window().ok_or(MacOsMainWindowError::MissingWindow)?;

    window.setStyleMask(window.styleMask() | NSWindowStyleMask::FullSizeContentView);
    window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
    window.setTitlebarAppearsTransparent(true);
    window.setTitlebarSeparatorStyle(NSTitlebarSeparatorStyle::None);
    if !window
        .styleMask()
        .contains(NSWindowStyleMask::FullSizeContentView)
        || window.titleVisibility() != NSWindowTitleVisibility::Hidden
        || !window.titlebarAppearsTransparent()
    {
        return Err(MacOsMainWindowError::ConfigurationRejected);
    }
    Ok(())
}
