use std::{ffi::c_void, ptr::NonNull};

use objc2::{MainThreadMarker, MainThreadOnly, rc::Retained};
use objc2_app_kit::{
    NSButton, NSLayoutConstraint, NSTitlebarSeparatorStyle, NSToolbar, NSToolbarDisplayMode,
    NSView, NSWindowButton, NSWindowStyleMask, NSWindowTitleVisibility, NSWindowToolbarStyle,
};
use objc2_foundation::{NSContainsRect, NSPoint, NSRect, NSSize};
use thiserror::Error;

const TRAFFIC_LIGHT_LEFT: f64 = 36.0;
const TRAFFIC_LIGHT_SPACING: f64 = 22.0;
const TRAFFIC_LIGHT_TOP: f64 = 26.0;

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
    #[error("macOS did not provide the standard traffic-light controls")]
    MissingTrafficLights,
}

/// Extends the Slint content behind the native titlebar while retaining the
/// standard macOS traffic-light controls.
///
/// # Safety
///
/// `ns_view` must point to a live `NSView` from a raw AppKit window handle and
/// remain valid for the duration of this call.
pub unsafe fn configure_main_window(ns_view: NonNull<c_void>) -> Result<(), MacOsMainWindowError> {
    let mtm = MainThreadMarker::new().ok_or(MacOsMainWindowError::NotMainThread)?;
    let view = unsafe { Retained::<NSView>::retain(ns_view.as_ptr().cast()) }
        .ok_or(MacOsMainWindowError::MissingView)?;
    let window = view.window().ok_or(MacOsMainWindowError::MissingWindow)?;

    window.setStyleMask(window.styleMask() | NSWindowStyleMask::FullSizeContentView);
    window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
    window.setTitlebarAppearsTransparent(true);
    window.setTitlebarSeparatorStyle(NSTitlebarSeparatorStyle::None);
    let toolbar = window.toolbar().unwrap_or_else(|| {
        let toolbar = NSToolbar::init(NSToolbar::alloc(mtm));
        toolbar.setDisplayMode(NSToolbarDisplayMode::IconOnly);
        window.setToolbar(Some(&toolbar));
        toolbar
    });
    toolbar.setVisible(true);
    window.setToolbarStyle(NSWindowToolbarStyle::Unified);

    let close = window
        .standardWindowButton(NSWindowButton::CloseButton)
        .ok_or(MacOsMainWindowError::MissingTrafficLights)?;
    let minimize = window
        .standardWindowButton(NSWindowButton::MiniaturizeButton)
        .ok_or(MacOsMainWindowError::MissingTrafficLights)?;
    let zoom = window
        .standardWindowButton(NSWindowButton::ZoomButton)
        .ok_or(MacOsMainWindowError::MissingTrafficLights)?;

    let titlebar =
        unsafe { close.superview() }.ok_or(MacOsMainWindowError::MissingTrafficLights)?;
    let placements = [
        (&close, TRAFFIC_LIGHT_LEFT),
        (&minimize, TRAFFIC_LIGHT_LEFT + TRAFFIC_LIGHT_SPACING),
        (&zoom, TRAFFIC_LIGHT_LEFT + TRAFFIC_LIGHT_SPACING * 2.0),
    ];
    if placements
        .iter()
        .any(|(button, left)| !traffic_light_fits(titlebar.bounds(), button.frame().size, *left))
    {
        return Err(MacOsMainWindowError::ConfigurationRejected);
    }

    let close_frame = close.frame();
    let minimize_frame = minimize.frame();
    let zoom_frame = zoom.frame();
    let close_constraints = constrain_traffic_light(&close, &titlebar, TRAFFIC_LIGHT_LEFT);
    let minimize_constraints = constrain_traffic_light(
        &minimize,
        &titlebar,
        TRAFFIC_LIGHT_LEFT + TRAFFIC_LIGHT_SPACING,
    );
    let zoom_constraints = constrain_traffic_light(
        &zoom,
        &titlebar,
        TRAFFIC_LIGHT_LEFT + TRAFFIC_LIGHT_SPACING * 2.0,
    );
    titlebar.layoutSubtreeIfNeeded();

    if [&close, &minimize, &zoom]
        .into_iter()
        .any(|button| !NSContainsRect(titlebar.bounds(), button.frame()))
    {
        restore_native_layout(&close, close_constraints, close_frame);
        restore_native_layout(&minimize, minimize_constraints, minimize_frame);
        restore_native_layout(&zoom, zoom_constraints, zoom_frame);
        titlebar.layoutSubtreeIfNeeded();
        return Err(MacOsMainWindowError::ConfigurationRejected);
    }

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

fn traffic_light_fits(titlebar: NSRect, button: NSSize, left: f64) -> bool {
    let frame = NSRect::new(
        NSPoint::new(
            titlebar.origin.x + left,
            titlebar.origin.y + titlebar.size.height - TRAFFIC_LIGHT_TOP - button.height,
        ),
        button,
    );
    NSContainsRect(titlebar, frame)
}

fn constrain_traffic_light(
    button: &NSButton,
    titlebar: &NSView,
    left: f64,
) -> Option<[Retained<NSLayoutConstraint>; 2]> {
    if !button.translatesAutoresizingMaskIntoConstraints() {
        return None;
    }

    button.setTranslatesAutoresizingMaskIntoConstraints(false);
    let left_constraint = button
        .leftAnchor()
        .constraintEqualToAnchor_constant(&titlebar.leftAnchor(), left);
    let top_constraint = button
        .topAnchor()
        .constraintEqualToAnchor_constant(&titlebar.topAnchor(), TRAFFIC_LIGHT_TOP);
    left_constraint.setActive(true);
    top_constraint.setActive(true);
    Some([left_constraint, top_constraint])
}

fn restore_native_layout(
    button: &NSButton,
    constraints: Option<[Retained<NSLayoutConstraint>; 2]>,
    frame: NSRect,
) {
    let Some(constraints) = constraints else {
        return;
    };
    for constraint in constraints {
        constraint.setActive(false);
    }
    button.setTranslatesAutoresizingMaskIntoConstraints(true);
    button.setFrame(frame);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserved_top_inset_requires_the_extended_titlebar() {
        let button = NSSize::new(14.0, 14.0);
        let standard_titlebar = NSRect::new(NSPoint::ZERO, NSSize::new(920.0, 32.0));
        let extended_titlebar = NSRect::new(NSPoint::ZERO, NSSize::new(920.0, 52.0));

        assert!(!traffic_light_fits(
            standard_titlebar,
            button,
            TRAFFIC_LIGHT_LEFT
        ));
        assert!(traffic_light_fits(
            extended_titlebar,
            button,
            TRAFFIC_LIGHT_LEFT + TRAFFIC_LIGHT_SPACING * 2.0
        ));
    }
}
