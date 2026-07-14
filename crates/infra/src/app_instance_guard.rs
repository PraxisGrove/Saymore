use std::{fs, fs::File, path::Path};

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
}

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
        match file.try_lock_exclusive() {
            Ok(()) => Ok(Self { _file: file }),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                Err(AppInstanceGuardError::AlreadyRunning)
            }
            Err(error) => Err(AppInstanceGuardError::Unavailable(error.to_string())),
        }
    }
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
}
