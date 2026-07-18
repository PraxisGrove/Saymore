use objc2::{MainThreadMarker, sel};
use objc2_app_kit::{NSApplication, NSMenu, NSMenuItem};
use objc2_foundation::ns_string;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum MacOsApplicationMenuError {
    #[error("the application menu must be installed on the macOS main thread")]
    NotMainThread,
    #[error("macOS did not provide the application main menu")]
    MissingMainMenu,
}

/// Adds the standard Window menu to Winit's existing macOS application menu.
///
/// Winit owns the application menu, including Command-Q. This adapter extends
/// it with Command-W while leaving AppKit to route the close action to the key
/// window and the desktop window event handler.
pub fn install_macos_application_menu() -> Result<(), MacOsApplicationMenuError> {
    let mtm = MainThreadMarker::new().ok_or(MacOsApplicationMenuError::NotMainThread)?;
    let application = NSApplication::sharedApplication(mtm);
    let main_menu = application
        .mainMenu()
        .ok_or(MacOsApplicationMenuError::MissingMainMenu)?;

    let window_menu = NSMenu::new(mtm);
    window_menu.setTitle(ns_string!("Window"));
    let close_window = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            mtm.alloc(),
            ns_string!("Close Window"),
            Some(sel!(performClose:)),
            ns_string!("w"),
        )
    };
    window_menu.addItem(&close_window);

    let window_menu_item = NSMenuItem::new(mtm);
    window_menu_item.setSubmenu(Some(&window_menu));
    main_menu.addItem(&window_menu_item);
    application.setWindowsMenu(Some(&window_menu));
    Ok(())
}
