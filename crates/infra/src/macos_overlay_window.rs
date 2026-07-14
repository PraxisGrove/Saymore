use std::{ffi::c_void, ptr::NonNull};

use objc2::{MainThreadMarker, rc::Retained};
use objc2_app_kit::{
    NSEvent, NSFloatingWindowLevel, NSScreen, NSView, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSPointInRect};
use thiserror::Error;

const BOTTOM_MARGIN: f64 = 12.0;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MacOsOverlayWindowError {
    #[error("the overlay must be configured on the macOS main thread")]
    NotMainThread,
    #[error("the Slint native view is no longer available")]
    MissingView,
    #[error("the Slint native view is not attached to a window")]
    MissingWindow,
    #[error("macOS did not report an available screen")]
    MissingScreen,
}

/// Positions a Slint overlay using AppKit's logical multi-display coordinate
/// system.
///
/// # Safety
///
/// `ns_view` must point to a live `NSView` from a raw AppKit window handle and
/// remain valid for the duration of this call.
pub unsafe fn configure_overlay_window(
    ns_view: NonNull<c_void>,
) -> Result<(), MacOsOverlayWindowError> {
    let mtm = MainThreadMarker::new().ok_or(MacOsOverlayWindowError::NotMainThread)?;
    let view = unsafe { Retained::<NSView>::retain(ns_view.as_ptr().cast()) }
        .ok_or(MacOsOverlayWindowError::MissingView)?;
    let window = view
        .window()
        .ok_or(MacOsOverlayWindowError::MissingWindow)?;
    window.setStyleMask(overlay_style_mask(window.styleMask()));
    window.setHidesOnDeactivate(false);
    let mouse = NSEvent::mouseLocation();
    let screens = NSScreen::screens(mtm);
    let visible_frame = screens
        .iter()
        .find(|screen| NSPointInRect(mouse, screen.frame()))
        .map(|screen| screen.visibleFrame())
        .or_else(|| NSScreen::mainScreen(mtm).map(|screen| screen.visibleFrame()))
        .ok_or(MacOsOverlayWindowError::MissingScreen)?;
    let window_frame = window.frame();
    let origin = NSPoint::new(
        visible_frame.origin.x + (visible_frame.size.width - window_frame.size.width) / 2.0,
        visible_frame.origin.y + BOTTOM_MARGIN,
    );

    window.setFrameOrigin(origin);
    window.setLevel(NSFloatingWindowLevel);
    window.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::Transient,
    );
    Ok(())
}

fn overlay_style_mask(current: NSWindowStyleMask) -> NSWindowStyleMask {
    current | NSWindowStyleMask::NonactivatingPanel
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_windows_do_not_activate_the_application() {
        assert!(
            overlay_style_mask(NSWindowStyleMask::Borderless)
                .contains(NSWindowStyleMask::NonactivatingPanel)
        );
    }
}
