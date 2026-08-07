use std::{ffi::c_void, ptr::NonNull, sync::OnceLock};

use objc2::{
    MainThreadMarker, ffi, msg_send,
    rc::Retained,
    runtime::{AnyClass, AnyObject, ClassBuilder, Sel},
    sel,
};
use objc2_app_kit::{
    NSEvent, NSFloatingWindowLevel, NSScreen, NSView, NSWindow, NSWindowCollectionBehavior,
    NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSPointInRect};
use thiserror::Error;

const BOTTOM_MARGIN: f64 = 12.0;
static OVERLAY_PANEL_CLASS: OnceLock<&'static AnyClass> = OnceLock::new();

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
    #[error("macOS could not register the native overlay panel class")]
    PanelClassRegistrationFailed,
    #[error("the native overlay window cannot be converted to an NSPanel")]
    IncompatibleWindowClass,
}

/// Configures a Slint/Winit window as a nonactivating macOS overlay.
///
/// Expected callers are short-lived UI surfaces such as recording, success,
/// recovery, and microphone-status overlays. Each Winit `NSWindow` is converted
/// to a process-owned `NSPanel` subclass without adding instance variables. A
/// real panel is required for an inactive application to appear in another
/// application's full-screen Space.
///
/// The panel is positioned using AppKit's logical multi-display coordinate
/// system and configured to appear above normal application windows.
///
/// # Safety
///
/// `ns_view` must point to a live `NSView` from a raw AppKit window handle and
/// remain valid for the duration of this call.
pub unsafe fn configure_overlay_window(
    ns_view: NonNull<c_void>,
) -> Result<(), MacOsOverlayWindowError> {
    // SAFETY: the caller guarantees that `ns_view` remains live for this call.
    let (mtm, window) = unsafe { overlay_window(ns_view)? };
    convert_to_panel(&window)?;
    window.setStyleMask(overlay_style_mask(window.styleMask()));
    window.setHidesOnDeactivate(false);
    window.setHasShadow(false);
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

unsafe fn overlay_window(
    ns_view: NonNull<c_void>,
) -> Result<(MainThreadMarker, Retained<objc2_app_kit::NSWindow>), MacOsOverlayWindowError> {
    let mtm = MainThreadMarker::new().ok_or(MacOsOverlayWindowError::NotMainThread)?;
    let view = unsafe { Retained::<NSView>::retain(ns_view.as_ptr().cast()) }
        .ok_or(MacOsOverlayWindowError::MissingView)?;
    let window = view
        .window()
        .ok_or(MacOsOverlayWindowError::MissingWindow)?;
    Ok((mtm, window))
}

fn convert_to_panel(window: &NSWindow) -> Result<(), MacOsOverlayWindowError> {
    let panel_class = overlay_panel_class()?;
    let current_class = window.class();
    if current_class == panel_class {
        return Ok(());
    }
    if current_class.instance_size() != panel_class.instance_size() {
        return Err(MacOsOverlayWindowError::IncompatibleWindowClass);
    }

    let window = std::ptr::from_ref(window).cast_mut().cast::<AnyObject>();
    // SAFETY: both classes have the same runtime instance size, the registered
    // panel subclass adds no ivars, and conversion runs only on the main thread
    // before AppKit presents the window. The content view and delegate remain
    // owned by the unchanged NSWindow storage.
    let previous = unsafe { ffi::object_setClass(window, panel_class) };
    if std::ptr::eq(previous, current_class) {
        Ok(())
    } else {
        Err(MacOsOverlayWindowError::IncompatibleWindowClass)
    }
}

fn overlay_panel_class() -> Result<&'static AnyClass, MacOsOverlayWindowError> {
    if let Some(class) = OVERLAY_PANEL_CLASS.get().copied() {
        return Ok(class);
    }
    if let Some(class) = AnyClass::get(c"SaymoreOverlayPanel") {
        let _ = OVERLAY_PANEL_CLASS.set(class);
        return Ok(class);
    }
    let superclass =
        AnyClass::get(c"NSPanel").ok_or(MacOsOverlayWindowError::PanelClassRegistrationFailed)?;
    let mut builder = ClassBuilder::new(c"SaymoreOverlayPanel", superclass)
        .ok_or(MacOsOverlayWindowError::PanelClassRegistrationFailed)?;
    // SAFETY: the selector ABI is void(receiver, selector, sender), and the
    // registered function uses that exact Objective-C calling convention.
    unsafe {
        builder.add_method(
            sel!(makeKeyAndOrderFront:),
            show_panel_without_stealing_focus as unsafe extern "C-unwind" fn(_, _, *mut AnyObject),
        );
    }
    let class = builder.register();
    let _ = OVERLAY_PANEL_CLASS.set(class);
    Ok(class)
}

unsafe extern "C-unwind" fn show_panel_without_stealing_focus(
    window: &AnyObject,
    _command: Sel,
    _sender: *mut AnyObject,
) {
    let _: () = unsafe { msg_send![window, orderFrontRegardless] };
}

fn overlay_style_mask(current: NSWindowStyleMask) -> NSWindowStyleMask {
    current | NSWindowStyleMask::NonactivatingPanel
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_windows_request_nonactivating_panel_behavior() {
        assert!(
            overlay_style_mask(NSWindowStyleMask::Borderless)
                .contains(NSWindowStyleMask::NonactivatingPanel)
        );
    }

    #[test]
    fn overlay_runtime_class_is_a_real_panel_without_extra_storage() -> Result<(), &'static str> {
        let panel = overlay_panel_class().map_err(|_| "the panel class should register")?;
        let superclass = panel
            .superclass()
            .ok_or("the panel should have a superclass")?;
        if superclass.name() != c"NSPanel" {
            return Err("the overlay class should inherit from NSPanel");
        }
        if panel.instance_size() != superclass.instance_size() {
            return Err("the overlay class must not add instance storage");
        }
        Ok(())
    }
}
