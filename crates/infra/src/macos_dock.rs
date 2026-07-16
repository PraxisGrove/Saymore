use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum MacOsDockError {
    #[error("the Dock preference must be changed on the macOS main thread")]
    NotMainThread,
    #[error("macOS rejected the requested Dock visibility")]
    Rejected,
}

pub fn dock_is_visible() -> Result<bool, MacOsDockError> {
    let mtm = MainThreadMarker::new().ok_or(MacOsDockError::NotMainThread)?;
    Ok(NSApplication::sharedApplication(mtm).activationPolicy()
        == NSApplicationActivationPolicy::Regular)
}

pub fn set_dock_visible(visible: bool) -> Result<(), MacOsDockError> {
    let mtm = MainThreadMarker::new().ok_or(MacOsDockError::NotMainThread)?;
    let application = NSApplication::sharedApplication(mtm);
    let policy = if visible {
        NSApplicationActivationPolicy::Regular
    } else {
        NSApplicationActivationPolicy::Accessory
    };
    if application.setActivationPolicy(policy) {
        Ok(())
    } else {
        Err(MacOsDockError::Rejected)
    }
}

pub fn activate_application() -> Result<(), MacOsDockError> {
    let mtm = MainThreadMarker::new().ok_or(MacOsDockError::NotMainThread)?;
    NSApplication::sharedApplication(mtm).activate();
    Ok(())
}
