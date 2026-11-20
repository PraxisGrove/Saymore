use async_trait::async_trait;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct UpdatePackage {
    pub version: String,
    pub file_name: String,
    pub download_url: String,
    pub checksums_url: String,
    pub size: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateProgress {
    Downloading { received: u64, total: u64 },
    Verifying,
}

#[derive(Clone, Default)]
pub struct UpdateCancellation(Arc<AtomicBool>);

impl UpdateCancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("update service unavailable")]
    Network,
    #[error("no supported installer in this release")]
    Unsupported,
    #[error("invalid update metadata")]
    Metadata,
    #[error("update checksum or size mismatch")]
    Integrity,
    #[error("could not save update")]
    Storage,
    #[error("download cancelled")]
    Cancelled,
}

/// Finds stable releases and saves verified installers. Implementations must report
/// byte progress, honor cancellation, and never return an incomplete installer.
/// Downloading does not execute the installer or replace the running application.
#[async_trait]
pub trait SoftwareUpdater: Send + Sync {
    async fn latest(&self, current_version: &str) -> Result<Option<UpdatePackage>, UpdateError>;
    async fn download(
        &self,
        package: &UpdatePackage,
        cancellation: UpdateCancellation,
        progress: Arc<dyn Fn(UpdateProgress) + Send + Sync>,
    ) -> Result<PathBuf, UpdateError>;
}

pub fn is_newer_release(remote: &str, current: &str) -> bool {
    match (release_version(remote), release_version(current)) {
        (Some(remote), Some(current)) => remote > current,
        _ => false,
    }
}

pub fn release_version(value: &str) -> Option<[u64; 3]> {
    let parts = value
        .strip_prefix('v')
        .unwrap_or(value)
        .split('.')
        .collect::<Vec<_>>();
    if parts.len() != 3 {
        return None;
    }
    let mut numbers = [0; 3];
    for (number, part) in numbers.iter_mut().zip(parts) {
        if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        *number = part.parse().ok()?;
    }
    Some(numbers)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_newer_stable_versions_are_updates() {
        assert!(is_newer_release("v0.2.11", "0.2.10"));
        for version in ["v0.2.10", "v0.2.9", "v0.2.11-beta", "v2", "../2.3.4"] {
            assert!(!is_newer_release(version, "0.2.10"));
        }
    }
}
