use objc2::available;
use objc2_service_management::{SMAppService, SMAppServiceStatus};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchAtLoginStatus {
    Disabled,
    Enabled,
    RequiresApproval,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MacOsLaunchAtLoginError {
    #[error("launch at login requires macOS 13 or newer")]
    Unsupported,
    #[error("macOS could not update launch at login: {0}")]
    System(String),
}

pub fn launch_at_login_status() -> Result<LaunchAtLoginStatus, MacOsLaunchAtLoginError> {
    if !available!(macos = 13.0) {
        return Err(MacOsLaunchAtLoginError::Unsupported);
    }
    let service = unsafe { SMAppService::mainAppService() };
    Ok(match unsafe { service.status() } {
        SMAppServiceStatus::Enabled => LaunchAtLoginStatus::Enabled,
        SMAppServiceStatus::RequiresApproval => LaunchAtLoginStatus::RequiresApproval,
        SMAppServiceStatus::NotRegistered | SMAppServiceStatus::NotFound => {
            LaunchAtLoginStatus::Disabled
        }
        _ => LaunchAtLoginStatus::Disabled,
    })
}

pub fn set_launch_at_login(enabled: bool) -> Result<(), MacOsLaunchAtLoginError> {
    if !available!(macos = 13.0) {
        return Err(MacOsLaunchAtLoginError::Unsupported);
    }
    let service = unsafe { SMAppService::mainAppService() };
    let result = if enabled {
        unsafe { service.registerAndReturnError() }
    } else {
        unsafe { service.unregisterAndReturnError() }
    };
    result
        .map_err(|error| MacOsLaunchAtLoginError::System(error.localizedDescription().to_string()))
}
