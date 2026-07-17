use std::{
    collections::HashSet,
    fs,
    fs::File,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

#[cfg(target_os = "windows")]
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use fs2::FileExt;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppInstanceGuardError {
    #[error("Saymore is already running")]
    AlreadyRunning,
    #[error("application instance lock is unavailable: {0}")]
    Unavailable(String),
}

pub struct AppInstanceGuard {
    _file: File,
    path: PathBuf,
    #[cfg(target_os = "windows")]
    activation_event: Option<windows::Win32::Foundation::HANDLE>,
    #[cfg(target_os = "windows")]
    activation_listener: Option<WindowsActivationListener>,
}

static HELD_INSTANCE_LOCKS: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();

impl AppInstanceGuard {
    pub fn acquire(path: &Path) -> Result<Self, AppInstanceGuardError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| AppInstanceGuardError::Unavailable(error.to_string()))?;
        }
        let file = File::options()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)
            .map_err(|error| AppInstanceGuardError::Unavailable(error.to_string()))?;
        let canonical_path = path
            .canonicalize()
            .map_err(|error| AppInstanceGuardError::Unavailable(error.to_string()))?;
        #[cfg(target_os = "windows")]
        let activation_event = acquire_activation_event(&canonical_path)?;
        let mut held = HELD_INSTANCE_LOCKS
            .get_or_init(|| Mutex::new(HashSet::new()))
            .lock()
            .map_err(|_| {
                AppInstanceGuardError::Unavailable(
                    "application instance lock registry was poisoned".to_owned(),
                )
            })?;
        if held.contains(&canonical_path) {
            return Err(AppInstanceGuardError::AlreadyRunning);
        }
        match file.try_lock_exclusive() {
            Ok(()) => {
                held.insert(canonical_path.clone());
                Ok(Self {
                    _file: file,
                    path: canonical_path,
                    #[cfg(target_os = "windows")]
                    activation_event: Some(activation_event),
                    #[cfg(target_os = "windows")]
                    activation_listener: None,
                })
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                Err(AppInstanceGuardError::AlreadyRunning)
            }
            Err(error) => Err(AppInstanceGuardError::Unavailable(error.to_string())),
        }
    }

    #[cfg(target_os = "windows")]
    pub fn listen_for_activation(
        &mut self,
        callback: impl Fn() + Send + 'static,
    ) -> Result<(), AppInstanceGuardError> {
        let event = self.activation_event.take().ok_or_else(|| {
            AppInstanceGuardError::Unavailable(
                "application activation listener is already installed".to_owned(),
            )
        })?;
        self.activation_listener = Some(WindowsActivationListener::start(event, callback)?);
        Ok(())
    }
}

impl Drop for AppInstanceGuard {
    fn drop(&mut self) {
        #[cfg(target_os = "windows")]
        drop(self.activation_listener.take());
        #[cfg(target_os = "windows")]
        if let Some(event) = self.activation_event.take() {
            // SAFETY: the event handle was created by CreateEventW and is still owned here.
            let _ = unsafe { windows::Win32::Foundation::CloseHandle(event) };
        }
        let Some(held) = HELD_INSTANCE_LOCKS.get() else {
            return;
        };
        match held.lock() {
            Ok(mut held) => {
                held.remove(&self.path);
            }
            Err(poisoned) => {
                poisoned.into_inner().remove(&self.path);
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn acquire_activation_event(
    path: &Path,
) -> Result<windows::Win32::Foundation::HANDLE, AppInstanceGuardError> {
    use windows::{
        Win32::{
            Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError},
            System::Threading::{CreateEventW, SetEvent},
        },
        core::PCWSTR,
    };

    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    let name = format!("Local\\Saymore.Activate.{:016x}", hasher.finish());
    let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    // SAFETY: the name is a stable, NUL-terminated UTF-16 slice for this call.
    let event = unsafe { CreateEventW(None, false, false, PCWSTR(name.as_ptr())) }
        .map_err(|error| AppInstanceGuardError::Unavailable(error.to_string()))?;
    // GetLastError immediately after CreateEventW identifies an existing named event.
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        // SAFETY: event is a valid handle returned by CreateEventW.
        let signal = unsafe { SetEvent(event) };
        // SAFETY: the duplicate instance owns this handle and closes it exactly once.
        let _ = unsafe { CloseHandle(event) };
        return match signal {
            Ok(()) => Err(AppInstanceGuardError::AlreadyRunning),
            Err(error) => Err(AppInstanceGuardError::Unavailable(error.to_string())),
        };
    }
    Ok(event)
}

#[cfg(target_os = "windows")]
struct WindowsActivationListener {
    shutdown: windows::Win32::Foundation::HANDLE,
    worker: Option<std::thread::JoinHandle<()>>,
}

#[cfg(target_os = "windows")]
impl WindowsActivationListener {
    fn start(
        activation: windows::Win32::Foundation::HANDLE,
        callback: impl Fn() + Send + 'static,
    ) -> Result<Self, AppInstanceGuardError> {
        use windows::Win32::System::Threading::CreateEventW;

        // SAFETY: an unnamed event has no pointer inputs and returns an owned handle.
        let shutdown = unsafe { CreateEventW(None, false, false, None) }
            .map_err(|error| AppInstanceGuardError::Unavailable(error.to_string()))?;
        let activation_value = activation.0 as isize;
        let shutdown_value = shutdown.0 as isize;
        let worker = std::thread::Builder::new()
            .name("saymore-instance-activation".to_owned())
            .spawn(move || {
                use windows::Win32::Foundation::HANDLE;

                activation_loop(
                    HANDLE(activation_value as *mut _),
                    HANDLE(shutdown_value as *mut _),
                    callback,
                );
            })
            .map_err(|error| {
                // SAFETY: thread creation failed, so both handles remain owned here.
                let _ = unsafe { windows::Win32::Foundation::CloseHandle(activation) };
                let _ = unsafe { windows::Win32::Foundation::CloseHandle(shutdown) };
                AppInstanceGuardError::Unavailable(error.to_string())
            })?;
        Ok(Self {
            shutdown,
            worker: Some(worker),
        })
    }
}

#[cfg(target_os = "windows")]
impl Drop for WindowsActivationListener {
    fn drop(&mut self) {
        // SAFETY: shutdown remains valid until the worker observes it and exits.
        let _ = unsafe { windows::Win32::System::Threading::SetEvent(self.shutdown) };
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(target_os = "windows")]
fn activation_loop(
    activation: windows::Win32::Foundation::HANDLE,
    shutdown: windows::Win32::Foundation::HANDLE,
    callback: impl Fn() + Send + 'static,
) {
    use windows::Win32::{
        Foundation::{CloseHandle, WAIT_OBJECT_0},
        System::Threading::{INFINITE, WaitForMultipleObjects},
    };

    loop {
        // SAFETY: both handles remain valid until this loop exits.
        let result = unsafe { WaitForMultipleObjects(&[activation, shutdown], false, INFINITE) };
        if result == WAIT_OBJECT_0 {
            callback();
        } else {
            break;
        }
    }
    // SAFETY: the worker owns both handles after it starts and closes each exactly once.
    let _ = unsafe { CloseHandle(activation) };
    let _ = unsafe { CloseHandle(shutdown) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permits_only_one_guard_for_a_lock_file() {
        let Ok(directory) = tempfile::tempdir() else {
            panic!("temporary directory should be available");
        };
        let path = directory.path().join("instance.lock");
        let Ok(first) = AppInstanceGuard::acquire(&path) else {
            panic!("the first instance guard should acquire the lock");
        };
        assert!(matches!(
            AppInstanceGuard::acquire(&path),
            Err(AppInstanceGuardError::AlreadyRunning)
        ));
        drop(first);
        assert!(AppInstanceGuard::acquire(&path).is_ok());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn second_guard_notifies_the_existing_windows_instance() {
        let Ok(directory) = tempfile::tempdir() else {
            panic!("temporary directory should be available");
        };
        let path = directory.path().join("instance.lock");
        let Ok(mut first) = AppInstanceGuard::acquire(&path) else {
            panic!("the first instance guard should acquire the lock");
        };
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        assert!(
            first
                .listen_for_activation(move || {
                    let _ = sender.try_send(());
                })
                .is_ok()
        );

        assert!(matches!(
            AppInstanceGuard::acquire(&path),
            Err(AppInstanceGuardError::AlreadyRunning)
        ));
        assert_eq!(
            Ok(()),
            receiver.recv_timeout(std::time::Duration::from_secs(2))
        );
    }
}
