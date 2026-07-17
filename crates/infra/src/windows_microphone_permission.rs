use std::{
    cell::Cell,
    future::{Future, IntoFuture},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    thread,
};

use template_app::{MicrophoneAuthorization, MicrophonePermissionProvider};
use thiserror::Error;
use windows::{
    Security::Authorization::AppCapabilityAccess::{AppCapability, AppCapabilityAccessStatus},
    Win32::{
        System::WinRT::{RO_INIT_SINGLETHREADED, RoInitialize},
        UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
    },
    core::{HSTRING, PCWSTR},
};

thread_local! {
    static WINRT_INITIALIZED: Cell<bool> = const { Cell::new(false) };
}

#[derive(Debug, Clone, Copy, Default)]
pub struct WindowsMicrophonePermission;

impl MicrophonePermissionProvider for WindowsMicrophonePermission {
    fn authorization(&self) -> MicrophoneAuthorization {
        query_authorization(false)
    }

    fn request_authorization(&self) -> MicrophoneAuthorization {
        query_authorization(true)
    }
}

fn query_authorization(request: bool) -> MicrophoneAuthorization {
    if let Err(error) = ensure_winrt_initialized() {
        tracing::warn!(event = "microphone.winrt_initialize_failed", reason = %error);
        return MicrophoneAuthorization::Restricted;
    }
    let result = AppCapability::Create(&HSTRING::from("microphone")).and_then(|capability| {
        if request {
            block_on(capability.RequestAccessAsync()?.into_future())
        } else {
            capability.CheckAccess()
        }
    });
    match result {
        Ok(status) => map_status(status),
        Err(error) => {
            // Unpackaged Win32 applications cannot observe every policy source on every
            // supported Windows build. API absence/failure is restricted, never granted.
            tracing::warn!(event = "microphone.authorization_query_failed", reason = %error);
            MicrophoneAuthorization::Restricted
        }
    }
}

fn ensure_winrt_initialized() -> windows::core::Result<()> {
    WINRT_INITIALIZED.with(|initialized| {
        if initialized.get() {
            return Ok(());
        }
        match unsafe { RoInitialize(RO_INIT_SINGLETHREADED) } {
            Ok(()) => {
                initialized.set(true);
                Ok(())
            }
            Err(error) if error.code().0 == 0x80010106_u32 as i32 => {
                // Another component already selected the apartment; WinRT is still usable.
                initialized.set(true);
                Ok(())
            }
            Err(error) => Err(error),
        }
    })
}

fn map_status(status: AppCapabilityAccessStatus) -> MicrophoneAuthorization {
    match status {
        AppCapabilityAccessStatus::Allowed => MicrophoneAuthorization::Granted,
        AppCapabilityAccessStatus::DeniedByUser => MicrophoneAuthorization::Denied,
        AppCapabilityAccessStatus::UserPromptRequired => MicrophoneAuthorization::NotDetermined,
        AppCapabilityAccessStatus::DeniedBySystem | AppCapabilityAccessStatus::NotDeclaredByApp => {
            MicrophoneAuthorization::Restricted
        }
        _ => MicrophoneAuthorization::Restricted,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WindowsMicrophoneSettingsError {
    #[error("Windows could not open microphone privacy settings (ShellExecuteW result {0})")]
    LaunchFailed(isize),
}

pub fn open_windows_microphone_privacy_settings() -> Result<(), WindowsMicrophoneSettingsError> {
    let operation = wide("open");
    let location = wide("ms-settings:privacy-microphone");
    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(operation.as_ptr()),
            PCWSTR(location.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    if result.0 as isize > 32 {
        Ok(())
    } else {
        Err(WindowsMicrophoneSettingsError::LaunchFailed(
            result.0 as isize,
        ))
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn block_on<F: Future>(future: F) -> F::Output {
    struct ThreadWake(thread::Thread);

    impl Wake for ThreadWake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }

    let waker = Waker::from(Arc::new(ThreadWake(thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        match Pin::as_mut(&mut future).poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => thread::park(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Ole::{OleInitialize, OleUninitialize};

    #[test]
    fn maps_documented_capability_states_without_assuming_access() {
        assert_eq!(
            MicrophoneAuthorization::Granted,
            map_status(AppCapabilityAccessStatus::Allowed)
        );
        assert_eq!(
            MicrophoneAuthorization::Denied,
            map_status(AppCapabilityAccessStatus::DeniedByUser)
        );
        assert_eq!(
            MicrophoneAuthorization::NotDetermined,
            map_status(AppCapabilityAccessStatus::UserPromptRequired)
        );
        assert_eq!(
            MicrophoneAuthorization::Restricted,
            map_status(AppCapabilityAccessStatus::DeniedBySystem)
        );
    }

    #[test]
    fn winrt_initialization_preserves_ui_thread_ole_compatibility() {
        let result = thread::spawn(|| {
            ensure_winrt_initialized()?;
            unsafe { OleInitialize(None)? };
            unsafe { OleUninitialize() };
            Ok::<(), windows::core::Error>(())
        })
        .join()
        .ok()
        .and_then(Result::ok);

        assert_eq!(Some(()), result);
    }
}
