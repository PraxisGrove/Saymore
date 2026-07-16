use std::cell::RefCell;

use objc2::runtime::{AnyClass, AnyObject, Bool, ClassBuilder, Sel};
use objc2::{MainThreadMarker, sel};
use objc2_app_kit::NSApplication;
use thiserror::Error;

const REOPEN_DELEGATE_CLASS: &std::ffi::CStr = c"SaymoreReopenApplicationDelegate";

thread_local! {
    static SHOW_MAIN_WINDOW: RefCell<Option<Box<dyn Fn()>>> = const { RefCell::new(None) };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum MacOsApplicationReopenError {
    #[error("the application reopen handler must be installed on the macOS main thread")]
    NotMainThread,
    #[error("macOS did not provide an application delegate")]
    MissingDelegate,
    #[error("the application reopen handler is already installed")]
    AlreadyInstalled,
    #[error("the application delegate could not be extended")]
    SubclassCreationFailed,
}

/// Restores the main window when a running application is reopened from the Dock.
pub struct MacOsApplicationReopenHandler;

impl MacOsApplicationReopenHandler {
    pub fn install(
        show_main_window: impl Fn() + 'static,
    ) -> Result<Self, MacOsApplicationReopenError> {
        let mtm = MainThreadMarker::new().ok_or(MacOsApplicationReopenError::NotMainThread)?;
        let application = NSApplication::sharedApplication(mtm);
        let delegate = application
            .delegate()
            .ok_or(MacOsApplicationReopenError::MissingDelegate)?;
        let delegate: &AnyObject = unsafe { &*(delegate.as_ref() as *const _) };
        if delegate.class().name() == REOPEN_DELEGATE_CLASS {
            return Err(MacOsApplicationReopenError::AlreadyInstalled);
        }
        let subclass = reopen_delegate_subclass(delegate.class())?;
        SHOW_MAIN_WINDOW.with(|callback| callback.replace(Some(Box::new(show_main_window))));
        // SAFETY: `subclass` directly extends the delegate's current class, adds no ivars,
        // and its only override matches the NSApplicationDelegate method signature.
        unsafe { AnyObject::set_class(delegate, subclass) };
        Ok(Self)
    }
}

fn reopen_delegate_subclass(
    superclass: &AnyClass,
) -> Result<&'static AnyClass, MacOsApplicationReopenError> {
    if let Some(class) = AnyClass::get(REOPEN_DELEGATE_CLASS) {
        return class
            .superclass()
            .filter(|parent| *parent == superclass)
            .map(|_| class)
            .ok_or(MacOsApplicationReopenError::SubclassCreationFailed);
    }
    let mut builder = ClassBuilder::new(REOPEN_DELEGATE_CLASS, superclass)
        .ok_or(MacOsApplicationReopenError::SubclassCreationFailed)?;
    // SAFETY: The function signature exactly matches
    // applicationShouldHandleReopen:hasVisibleWindows:.
    unsafe {
        builder.add_method(
            sel!(applicationShouldHandleReopen:hasVisibleWindows:),
            handle_reopen as extern "C-unwind" fn(_, _, _, _) -> _,
        );
    }
    Ok(builder.register())
}

extern "C-unwind" fn handle_reopen(
    _delegate: &AnyObject,
    _selector: Sel,
    _application: &NSApplication,
    has_visible_windows: Bool,
) -> Bool {
    if has_visible_windows.is_false() {
        SHOW_MAIN_WINDOW.with(|callback| {
            if let Some(callback) = callback.borrow().as_ref() {
                callback();
            }
        });
    }
    Bool::YES
}

impl Drop for MacOsApplicationReopenHandler {
    fn drop(&mut self) {
        SHOW_MAIN_WINDOW.with(|callback| callback.replace(None));
    }
}
