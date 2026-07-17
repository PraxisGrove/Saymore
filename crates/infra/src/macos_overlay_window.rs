use std::{cell::Cell, ffi::c_void, ptr::NonNull, sync::OnceLock};

use objc2::{
    ClassType, MainThreadMarker, ffi, msg_send,
    rc::Retained,
    runtime::{AnyClass, AnyObject, Imp, Sel},
    sel,
};
use objc2_app_kit::{
    NSEvent, NSFloatingWindowLevel, NSScreen, NSView, NSWindow, NSWindowCollectionBehavior,
    NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSPointInRect};
use thiserror::Error;

const BOTTOM_MARGIN: f64 = 12.0;
static HOOKED_WINIT_CLASS: OnceLock<&'static AnyClass> = OnceLock::new();
static NONACTIVATING_MARKER_KEY: u8 = 0;

thread_local! {
    static OVERLAY_PRESENTATION_ACTIVE: Cell<bool> = const { Cell::new(false) };
}

struct OverlayPresentationGuard<'a> {
    presenting: &'a Cell<bool>,
}

impl<'a> OverlayPresentationGuard<'a> {
    fn enter(presenting: &'a Cell<bool>) -> Option<Self> {
        if presenting.replace(true) {
            None
        } else {
            Some(Self { presenting })
        }
    }
}

impl Drop for OverlayPresentationGuard<'_> {
    fn drop(&mut self) {
        self.presenting.set(false);
    }
}

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
    #[error("the native overlay window was not backed by the expected winit class")]
    UnexpectedWindowClass,
    #[error("macOS could not install the nonactivating overlay presentation hook")]
    HookRegistrationFailed,
}

/// Configures a Slint/Winit window as a nonactivating macOS overlay.
///
/// Expected callers are short-lived UI surfaces such as recording, success,
/// recovery, and microphone-status overlays. The first call installs a
/// process-wide override on the active Winit `NSWindow` subclass. The override
/// forwards normal windows to Winit's inherited presentation behavior and uses
/// nonactivating presentation only for windows carrying this function's
/// lifecycle-bound marker. Calls for a different Winit window subclass are
/// rejected rather than changing another backend implicitly.
///
/// The window is also positioned using AppKit's logical multi-display
/// coordinate system and configured to appear above normal application windows.
///
/// # Safety
///
/// `ns_view` must point to a live `NSView` from a raw AppKit window handle and
/// remain valid for the duration of this call.
pub unsafe fn configure_overlay_window(
    ns_view: NonNull<c_void>,
) -> Result<(), MacOsOverlayWindowError> {
    let (mtm, window) = overlay_window(ns_view)?;
    install_nonactivating_presentation(&window)?;
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

fn overlay_window(
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

fn install_nonactivating_presentation(window: &NSWindow) -> Result<(), MacOsOverlayWindowError> {
    let winit_class = winit_window_class(window.class())?;
    install_presentation_hook(winit_class)?;
    let window = std::ptr::from_ref(window).cast_mut().cast::<AnyObject>();
    // The assign association points back to the window itself. It does not
    // create a retain cycle, and Objective-C removes it when the window dies.
    unsafe {
        ffi::objc_setAssociatedObject(
            window,
            nonactivating_marker_key(),
            window,
            ffi::OBJC_ASSOCIATION_ASSIGN,
        );
    }
    Ok(())
}

fn winit_window_class(
    mut class: &'static AnyClass,
) -> Result<&'static AnyClass, MacOsOverlayWindowError> {
    loop {
        let superclass = class
            .superclass()
            .ok_or(MacOsOverlayWindowError::UnexpectedWindowClass)?;
        if superclass == NSWindow::class() {
            return Ok(class);
        }
        class = superclass;
    }
}

fn install_presentation_hook(
    winit_class: &'static AnyClass,
) -> Result<(), MacOsOverlayWindowError> {
    if let Some(installed) = HOOKED_WINIT_CLASS.get().copied() {
        return if installed == winit_class {
            Ok(())
        } else {
            Err(MacOsOverlayWindowError::UnexpectedWindowClass)
        };
    }
    let selector = sel!(makeKeyAndOrderFront:);
    let method = winit_class
        .instance_method(selector)
        .ok_or(MacOsOverlayWindowError::HookRegistrationFailed)?;
    // The method comes from the live class object and its encoding is owned by
    // the Objective-C runtime for the lifetime of that class.
    let type_encoding = unsafe { ffi::method_getTypeEncoding(method) };
    if type_encoding.is_null() {
        return Err(MacOsOverlayWindowError::HookRegistrationFailed);
    }
    // Objective-C IMPs use this exact receiver/selector/sender ABI for a void
    // method with one object argument.
    let implementation: Imp = unsafe {
        std::mem::transmute::<unsafe extern "C-unwind" fn(&AnyObject, Sel, Option<&AnyObject>), Imp>(
            show_window_without_stealing_focus,
        )
    };
    // The class and type encoding are runtime-owned and remain valid after the
    // method is installed.
    let added = unsafe {
        ffi::class_addMethod(
            std::ptr::from_ref(winit_class).cast_mut(),
            selector,
            implementation,
            type_encoding,
        )
    };
    if !added.as_bool() {
        return Err(MacOsOverlayWindowError::HookRegistrationFailed);
    }
    if HOOKED_WINIT_CLASS.set(winit_class).is_ok() {
        Ok(())
    } else {
        HOOKED_WINIT_CLASS
            .get()
            .copied()
            .filter(|installed| *installed == winit_class)
            .map(|_| ())
            .ok_or(MacOsOverlayWindowError::UnexpectedWindowClass)
    }
}

fn nonactivating_marker_key() -> *const c_void {
    std::ptr::from_ref(&NONACTIVATING_MARKER_KEY).cast()
}

unsafe extern "C-unwind" fn show_window_without_stealing_focus(
    window: &AnyObject,
    _command: Sel,
    sender: Option<&AnyObject>,
) {
    // Associated objects are automatically cleared when their owner is
    // destroyed, so a later window at the same address cannot inherit the mark.
    let marker = unsafe {
        ffi::objc_getAssociatedObject(std::ptr::from_ref(window), nonactivating_marker_key())
    };
    let is_overlay = !marker.is_null();
    if is_overlay {
        let _ = OVERLAY_PRESENTATION_ACTIVE.try_with(|presenting| {
            let Some(_guard) = OverlayPresentationGuard::enter(presenting) else {
                return;
            };
            // Winit uses orderFront: for visible windows that must not become key.
            let _: () = unsafe { msg_send![window, orderFront: sender] };
        });
    } else if let Some(winit_class) = HOOKED_WINIT_CLASS.get().copied() {
        // Preserve Winit's normal key-window behavior for every unmarked window.
        let _: () = unsafe { msg_send![super(window, winit_class), makeKeyAndOrderFront: sender] };
    }
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
    fn nested_overlay_presentation_is_suppressed() {
        let presenting = Cell::new(false);
        let first = OverlayPresentationGuard::enter(&presenting);

        assert!(first.is_some());
        assert!(OverlayPresentationGuard::enter(&presenting).is_none());

        drop(first);
        assert!(OverlayPresentationGuard::enter(&presenting).is_some());
    }
}
