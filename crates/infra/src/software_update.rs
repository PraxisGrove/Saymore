mod download;
mod release;

use async_trait::async_trait;
use std::{path::PathBuf, sync::Arc, time::Duration};
use template_app::{
    SoftwareUpdater, UpdateCancellation, UpdateError, UpdatePackage, UpdateProgress,
};

const RELEASES_URL: &str = "https://api.github.com/repos/PraxisGrove/Saymore/releases/latest";
const DOWNLOAD_PREFIX: &str = "https://github.com/PraxisGrove/Saymore/releases/download/";

#[derive(Clone, Copy)]
pub enum UpdatePlatform {
    MacOs,
    Windows,
    Unsupported,
}

impl UpdatePlatform {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Unsupported
        }
    }
}

pub struct GithubSoftwareUpdater {
    client: reqwest::Client,
    directory: PathBuf,
    platform: UpdatePlatform,
}

impl GithubSoftwareUpdater {
    pub fn new(directory: PathBuf, platform: UpdatePlatform) -> Result<Self, UpdateError> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .user_agent("Saymore updater")
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(30))
            .https_only(true)
            .build()
            .map_err(|_| UpdateError::Network)?;
        Ok(Self {
            client,
            directory,
            platform,
        })
    }
}

#[async_trait]
impl SoftwareUpdater for GithubSoftwareUpdater {
    async fn latest(&self, current_version: &str) -> Result<Option<UpdatePackage>, UpdateError> {
        let response = self
            .client
            .get(RELEASES_URL)
            .timeout(Duration::from_secs(15))
            .send()
            .await
            .map_err(|_| UpdateError::Network)?
            .error_for_status()
            .map_err(|_| UpdateError::Network)?;
        let bytes = download::bounded_body(response, 2 * 1024 * 1024).await?;
        release::select(&bytes, current_version, self.platform)
    }

    async fn download(
        &self,
        package: &UpdatePackage,
        cancellation: UpdateCancellation,
        progress: Arc<dyn Fn(UpdateProgress) + Send + Sync>,
    ) -> Result<PathBuf, UpdateError> {
        release::validate(package)?;
        download::save(
            &self.client,
            &self.directory,
            package,
            cancellation,
            progress,
        )
        .await
    }
}
