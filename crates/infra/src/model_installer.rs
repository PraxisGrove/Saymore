use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    time::Duration,
};

use bzip2::read::BzDecoder;
use futures_util::StreamExt;
use reqwest::{Client, StatusCode, header::RANGE};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio_util::sync::CancellationToken;

mod manifests;
mod sense_voice_manifest;
mod verification;

use verification::{verification_marker_matches, write_verification_marker};

#[cfg(test)]
mod tests;

pub const PARAFORMER_MODEL_ID: &str = "paraformer-zh-en-int8";
pub const PARAFORMER_MODEL_REVISION: &str = "8e40c43232a1c5c66c82111efc5820d3accca11b";
pub const WHISPER_MODEL_ID: &str = "whisper-large-v3-turbo-int8";
pub const WHISPER_MODEL_REVISION: &str = "2ca6ff69fc878651b770880507669577ac41c2ff";
pub const QWEN3_ASR_MODEL_ID: &str = "qwen3-asr-1.7b-int8";
pub const QWEN3_ASR_MODEL_REVISION: &str = "cb045ad80b8970c9d411d463e5b78991a566596c";
pub const SENSE_VOICE_MODEL_ID: &str = "sense-voice-small-int8";
pub const SENSE_VOICE_MODEL_REVISION: &str = "2365baeacb507f821a0c8120fcee3d484dba7a07";
pub const PUNCTUATION_MODEL_ID: &str = "ct-transformer-zh-en-punctuation-int8";
pub const PUNCTUATION_MODEL_REVISION: &str = "2024-04-12-int8";
const DOWNLOAD_RETRIES: usize = 3;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const READ_TIMEOUT: Duration = Duration::from_secs(30);
const INSTALL_SPACE_MARGIN_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelDownloadProgress {
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ModelInstallInterruption {
    #[error("paused")]
    Paused,
    #[error("cancelled")]
    Cancelled,
}

#[derive(Clone, Default)]
pub struct ModelInstallControl {
    interruption: Arc<AtomicU8>,
    cancellation: CancellationToken,
}

impl ModelInstallControl {
    pub fn pause(&self) {
        let _ = self
            .interruption
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire);
        self.cancellation.cancel();
    }

    pub fn cancel(&self) {
        self.interruption.store(2, Ordering::Release);
        self.cancellation.cancel();
    }

    fn check(&self) -> Result<(), ModelInstallError> {
        match self.interruption.load(Ordering::Acquire) {
            1 => Err(ModelInstallError::Interrupted(
                ModelInstallInterruption::Paused,
            )),
            2 => Err(ModelInstallError::Interrupted(
                ModelInstallInterruption::Cancelled,
            )),
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Error)]
pub enum ModelInstallError {
    #[error("the model download failed: {0}")]
    Download(String),
    #[error("the downloaded model failed integrity validation: {0}")]
    Integrity(String),
    #[error("the model requires {required_bytes} bytes but only {available_bytes} are available")]
    InsufficientSpace {
        required_bytes: u64,
        available_bytes: u64,
    },
    #[error("the model filesystem operation failed: {0}")]
    Filesystem(String),
    #[error("the installed model could not be activated: {0}")]
    Activation(String),
    #[error("the model download was {0}")]
    Interrupted(ModelInstallInterruption),
}

impl ModelInstallError {
    fn retryable(&self) -> bool {
        matches!(self, Self::Download(_) | Self::Integrity(_))
    }
}

#[derive(Clone)]
struct ModelArtifact {
    remote_path: String,
    local_path: String,
    bytes: u64,
    sha256: String,
}

#[derive(Clone)]
struct ModelManifest {
    id: String,
    revision: String,
    base_url: String,
    downloads: Vec<ModelArtifact>,
    artifacts: Vec<ModelArtifact>,
    preparation: ModelPreparation,
}

#[derive(Clone)]
enum ModelPreparation {
    Direct,
    TarBzip2 {
        archive_local_path: String,
        member_path: String,
    },
}

impl ModelManifest {
    fn direct(id: &str, revision: &str, base_url: &str, artifacts: Vec<ModelArtifact>) -> Self {
        Self {
            id: id.to_owned(),
            revision: revision.to_owned(),
            base_url: base_url.to_owned(),
            downloads: artifacts.clone(),
            artifacts,
            preparation: ModelPreparation::Direct,
        }
    }

    fn total_bytes(&self) -> u64 {
        self.downloads.iter().map(|artifact| artifact.bytes).sum()
    }

    fn installed_bytes(&self) -> u64 {
        self.artifacts.iter().map(|artifact| artifact.bytes).sum()
    }
}

fn artifact(name: &str, bytes: u64, sha256: &str) -> ModelArtifact {
    artifact_at(name, name, bytes, sha256)
}

fn artifact_at(remote_path: &str, local_path: &str, bytes: u64, sha256: &str) -> ModelArtifact {
    ModelArtifact {
        remote_path: remote_path.to_owned(),
        local_path: local_path.to_owned(),
        bytes,
        sha256: sha256.to_owned(),
    }
}

pub struct VerifiedModelInstaller {
    models_root: PathBuf,
    client: Client,
    manifest: ModelManifest,
}

impl VerifiedModelInstaller {
    pub fn paraformer(models_root: PathBuf) -> Result<Self, ModelInstallError> {
        Self::new(models_root, ModelManifest::paraformer())
    }

    pub fn whisper_large_v3_turbo(models_root: PathBuf) -> Result<Self, ModelInstallError> {
        Self::new(models_root, ModelManifest::whisper_large_v3_turbo())
    }

    pub fn qwen3_asr_1_7b(models_root: PathBuf) -> Result<Self, ModelInstallError> {
        Self::new(models_root, ModelManifest::qwen3_asr_1_7b())
    }

    pub fn sense_voice_small(models_root: PathBuf) -> Result<Self, ModelInstallError> {
        Self::new(models_root, ModelManifest::sense_voice_small())
    }

    pub fn punctuation(models_root: PathBuf) -> Result<Self, ModelInstallError> {
        Self::new(models_root, ModelManifest::punctuation())
    }

    fn new(models_root: PathBuf, manifest: ModelManifest) -> Result<Self, ModelInstallError> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::limited(10))
            .connect_timeout(CONNECT_TIMEOUT)
            .read_timeout(READ_TIMEOUT)
            .build()
            .map_err(download_error)?;
        Ok(Self {
            models_root,
            client,
            manifest,
        })
    }

    pub fn model_directory(&self) -> PathBuf {
        self.models_root
            .join(&self.manifest.id)
            .join(&self.manifest.revision)
    }

    /// Exact number of bytes transferred for a fresh install of the pinned artifacts.
    pub fn download_size_bytes(&self) -> u64 {
        self.manifest.total_bytes()
    }

    /// Current logical size of all regular files in this model's installed directory.
    pub fn installed_size_bytes(&self) -> Result<u64, ModelInstallError> {
        crate::storage_usage::directory_usage_bytes(&self.model_directory())
            .map_err(filesystem_error)
    }

    pub fn partial_download_progress(&self) -> Result<ModelDownloadProgress, ModelInstallError> {
        Ok(ModelDownloadProgress {
            downloaded_bytes: staged_bytes(&self.staging_directory(), &self.manifest)?,
            total_bytes: self.manifest.total_bytes(),
        })
    }

    pub fn is_installed(&self) -> bool {
        let directory = self.model_directory();
        if verification_marker_matches(&directory, &self.manifest) {
            return true;
        }
        if !manifest_matches(&directory, &self.manifest) {
            return false;
        }
        if let Err(error) = write_verification_marker(&directory, &self.manifest) {
            tracing::warn!(
                event = "model.verification_marker_write_failed",
                model_id = %self.manifest.id,
                reason = %error
            );
        }
        true
    }

    pub async fn install(
        &self,
        on_progress: Arc<dyn Fn(ModelDownloadProgress) + Send + Sync>,
    ) -> Result<PathBuf, ModelInstallError> {
        self.install_with_control(on_progress, ModelInstallControl::default())
            .await
    }

    pub async fn install_with_control(
        &self,
        on_progress: Arc<dyn Fn(ModelDownloadProgress) + Send + Sync>,
        control: ModelInstallControl,
    ) -> Result<PathBuf, ModelInstallError> {
        let mut retries = 0;
        loop {
            control.check()?;
            match self.install_once(Arc::clone(&on_progress), &control).await {
                Ok(path) => return Ok(path),
                Err(error) if error.retryable() && retries < DOWNLOAD_RETRIES => {
                    retries += 1;
                    tracing::warn!(
                        event = "model.download_retry",
                        model_id = %self.manifest.id,
                        retry = retries,
                        reason = %error
                    );
                }
                Err(error) => return Err(error),
            }
        }
    }

    pub fn remove(&self) -> Result<(), ModelInstallError> {
        remove_directory_if_present(&self.model_directory())?;
        remove_directory_if_present(&self.staging_directory())?;
        remove_directory_if_present(&self.prepared_directory())
    }

    pub fn discard_partial_download(&self) -> Result<(), ModelInstallError> {
        remove_directory_if_present(&self.staging_directory())?;
        remove_directory_if_present(&self.prepared_directory())
    }

    async fn install_once(
        &self,
        on_progress: Arc<dyn Fn(ModelDownloadProgress) + Send + Sync>,
        control: &ModelInstallControl,
    ) -> Result<PathBuf, ModelInstallError> {
        control.check()?;
        if self.is_installed() {
            on_progress(ModelDownloadProgress {
                downloaded_bytes: self.manifest.total_bytes(),
                total_bytes: self.manifest.total_bytes(),
            });
            return Ok(self.model_directory());
        }
        fs::create_dir_all(&self.models_root).map_err(filesystem_error)?;
        let staging = self.staging_directory();
        fs::create_dir_all(&staging).map_err(filesystem_error)?;
        self.ensure_space(&staging)?;
        for artifact in &self.manifest.downloads {
            control.check()?;
            self.download_artifact(&staging, artifact, Arc::clone(&on_progress), control)
                .await?;
        }
        control.check()?;
        let prepared = self.prepare_activation(&staging)?;
        control.check()?;
        self.activate(&prepared)?;
        if prepared != staging {
            remove_directory_if_present(&staging)?;
        }
        Ok(self.model_directory())
    }

    async fn download_artifact(
        &self,
        staging: &Path,
        artifact: &ModelArtifact,
        on_progress: Arc<dyn Fn(ModelDownloadProgress) + Send + Sync>,
        control: &ModelInstallControl,
    ) -> Result<(), ModelInstallError> {
        control.check()?;
        let complete = staging.join(&artifact.local_path);
        if file_matches(&complete, artifact)? {
            self.report_progress(staging, &on_progress)?;
            return Ok(());
        }
        remove_file_if_present(&complete)?;
        let partial = complete.with_extension(format!(
            "{}part",
            complete
                .extension()
                .and_then(|extension| extension.to_str())
                .map(|extension| format!("{extension}."))
                .unwrap_or_default()
        ));
        if let Some(parent) = partial.parent() {
            fs::create_dir_all(parent).map_err(filesystem_error)?;
        }
        let existing = file_size(&partial)?;
        if existing > artifact.bytes {
            remove_file_if_present(&partial)?;
        }
        let existing = file_size(&partial)?;
        if existing == artifact.bytes {
            return self.finish_artifact(&partial, &complete, artifact);
        }

        let url = if artifact.remote_path.starts_with("https://") {
            artifact.remote_path.clone()
        } else {
            format!("{}/{}", self.manifest.base_url, artifact.remote_path)
        };
        let mut request = self.client.get(url);
        if existing > 0 {
            request = request.header(RANGE, format!("bytes={existing}-"));
        }
        let response = tokio::select! {
            () = control.cancellation.cancelled() => return control.check(),
            response = request.send() => response.map_err(download_error)?,
        };
        let append = existing > 0 && response.status() == StatusCode::PARTIAL_CONTENT;
        if !append && !response.status().is_success() {
            return Err(ModelInstallError::Download(format!(
                "{} returned HTTP {}",
                artifact.local_path,
                response.status()
            )));
        }
        let mut file = open_partial(&partial, append)?;
        let mut written = if append { existing } else { 0 };
        let mut stream = response.bytes_stream();
        loop {
            let chunk = tokio::select! {
                () = control.cancellation.cancelled() => return control.check(),
                chunk = stream.next() => chunk,
            };
            let Some(chunk) = chunk else {
                break;
            };
            let chunk = chunk.map_err(download_error)?;
            written = written.saturating_add(chunk.len() as u64);
            if written > artifact.bytes {
                return Err(ModelInstallError::Integrity(format!(
                    "{} exceeded its fixed size",
                    artifact.local_path
                )));
            }
            file.write_all(&chunk).map_err(filesystem_error)?;
            self.report_progress(staging, &on_progress)?;
            control.check()?;
        }
        file.sync_all().map_err(filesystem_error)?;
        drop(file);
        if written != artifact.bytes {
            return Err(ModelInstallError::Download(format!(
                "{} ended at {written} of {} bytes",
                artifact.local_path, artifact.bytes
            )));
        }
        self.finish_artifact(&partial, &complete, artifact)?;
        self.report_progress(staging, &on_progress)
    }

    fn finish_artifact(
        &self,
        partial: &Path,
        complete: &Path,
        artifact: &ModelArtifact,
    ) -> Result<(), ModelInstallError> {
        if !file_matches(partial, artifact)? {
            remove_file_if_present(partial)?;
            return Err(ModelInstallError::Integrity(format!(
                "{} SHA-256 does not match the pinned manifest",
                artifact.local_path
            )));
        }
        fs::rename(partial, complete).map_err(filesystem_error)
    }

    fn ensure_space(&self, staging: &Path) -> Result<(), ModelInstallError> {
        let downloaded = staged_bytes(staging, &self.manifest)?;
        let extraction_bytes = match &self.manifest.preparation {
            ModelPreparation::Direct => 0,
            ModelPreparation::TarBzip2 { .. } => self.manifest.installed_bytes(),
        };
        let required = self
            .manifest
            .total_bytes()
            .saturating_sub(downloaded)
            .saturating_add(extraction_bytes)
            .saturating_add(INSTALL_SPACE_MARGIN_BYTES);
        let available = fs2::available_space(&self.models_root).map_err(filesystem_error)?;
        if available < required {
            return Err(ModelInstallError::InsufficientSpace {
                required_bytes: required,
                available_bytes: available,
            });
        }
        Ok(())
    }

    fn report_progress(
        &self,
        staging: &Path,
        on_progress: &Arc<dyn Fn(ModelDownloadProgress) + Send + Sync>,
    ) -> Result<(), ModelInstallError> {
        on_progress(ModelDownloadProgress {
            downloaded_bytes: staged_bytes(staging, &self.manifest)?,
            total_bytes: self.manifest.total_bytes(),
        });
        Ok(())
    }

    fn activate(&self, staging: &Path) -> Result<(), ModelInstallError> {
        write_verification_marker(staging, &self.manifest)?;
        let target = self.model_directory();
        let parent = target.parent().ok_or_else(|| {
            ModelInstallError::Filesystem("the model path has no parent".to_owned())
        })?;
        fs::create_dir_all(parent).map_err(filesystem_error)?;
        let backup = self.models_root.join(format!(
            ".{}-{}.backup",
            self.manifest.id, self.manifest.revision
        ));
        remove_directory_if_present(&backup)?;
        if target.exists() {
            fs::rename(&target, &backup).map_err(filesystem_error)?;
        }
        if let Err(error) = fs::rename(staging, &target) {
            if backup.exists() {
                let _ = fs::rename(&backup, &target);
            }
            return Err(filesystem_error(error));
        }
        remove_directory_if_present(&backup)
    }

    fn prepare_activation(&self, staging: &Path) -> Result<PathBuf, ModelInstallError> {
        let ModelPreparation::TarBzip2 {
            archive_local_path,
            member_path,
        } = &self.manifest.preparation
        else {
            return Ok(staging.to_path_buf());
        };
        let prepared = self.prepared_directory();
        remove_directory_if_present(&prepared)?;
        fs::create_dir_all(&prepared).map_err(filesystem_error)?;
        let installed = self.manifest.artifacts.first().ok_or_else(|| {
            ModelInstallError::Integrity(
                "the archive manifest has no installed artifact".to_owned(),
            )
        })?;
        extract_tar_bzip2_member(
            &staging.join(archive_local_path),
            member_path,
            &prepared.join(&installed.local_path),
            installed.bytes,
        )?;
        if !manifest_matches(&prepared, &self.manifest) {
            remove_directory_if_present(&prepared)?;
            return Err(ModelInstallError::Integrity(
                "the extracted model does not match the pinned manifest".to_owned(),
            ));
        }
        Ok(prepared)
    }

    fn staging_directory(&self) -> PathBuf {
        self.models_root.join(format!(
            ".{}-{}.download",
            self.manifest.id, self.manifest.revision
        ))
    }

    fn prepared_directory(&self) -> PathBuf {
        self.models_root.join(format!(
            ".{}-{}.install",
            self.manifest.id, self.manifest.revision
        ))
    }

    #[cfg(test)]
    fn with_manifest(
        models_root: PathBuf,
        base_url: String,
        artifacts: Vec<ModelArtifact>,
    ) -> Result<Self, ModelInstallError> {
        Self::new(
            models_root,
            ModelManifest::direct("test-model", "test-revision", &base_url, artifacts),
        )
    }

    #[cfg(test)]
    fn with_archive_manifest(
        models_root: PathBuf,
        base_url: String,
        archive: ModelArtifact,
        member_path: String,
        installed: ModelArtifact,
    ) -> Result<Self, ModelInstallError> {
        Self::new(
            models_root,
            ModelManifest {
                id: "test-archive-model".to_owned(),
                revision: "test-revision".to_owned(),
                base_url,
                downloads: vec![archive.clone()],
                artifacts: vec![installed],
                preparation: ModelPreparation::TarBzip2 {
                    archive_local_path: archive.local_path,
                    member_path,
                },
            },
        )
    }
}

fn extract_tar_bzip2_member(
    archive_path: &Path,
    member_path: &str,
    destination: &Path,
    expected_bytes: u64,
) -> Result<(), ModelInstallError> {
    let archive_file = File::open(archive_path).map_err(filesystem_error)?;
    let decoder = BzDecoder::new(archive_file);
    let mut archive = tar::Archive::new(decoder);
    let mut found = false;
    for entry in archive.entries().map_err(filesystem_error)? {
        let mut entry = entry.map_err(filesystem_error)?;
        let path = entry.path().map_err(filesystem_error)?;
        if path != Path::new(member_path) {
            continue;
        }
        if found || !entry.header().entry_type().is_file() || entry.size() != expected_bytes {
            return Err(ModelInstallError::Integrity(
                "the punctuation archive contains an invalid model entry".to_owned(),
            ));
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(filesystem_error)?;
        }
        let mut output = File::create(destination).map_err(filesystem_error)?;
        std::io::copy(&mut entry, &mut output).map_err(filesystem_error)?;
        output.sync_all().map_err(filesystem_error)?;
        found = true;
    }
    if !found {
        return Err(ModelInstallError::Integrity(
            "the punctuation archive does not contain the pinned model entry".to_owned(),
        ));
    }
    Ok(())
}

fn open_partial(path: &Path, append: bool) -> Result<File, ModelInstallError> {
    OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(path)
        .map_err(filesystem_error)
}

fn staged_bytes(staging: &Path, manifest: &ModelManifest) -> Result<u64, ModelInstallError> {
    manifest
        .downloads
        .iter()
        .try_fold(0_u64, |total, artifact| {
            let complete = file_size(&staging.join(&artifact.local_path))?;
            let partial = file_size(&staging.join(&artifact.local_path).with_extension(format!(
                "{}part",
                Path::new(&artifact.local_path)
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .map(|extension| format!("{extension}."))
                    .unwrap_or_default()
            )))?;
            Ok(total.saturating_add(complete.max(partial).min(artifact.bytes)))
        })
}

fn manifest_matches(directory: &Path, manifest: &ModelManifest) -> bool {
    manifest.artifacts.iter().all(|artifact| {
        file_matches(&directory.join(&artifact.local_path), artifact).unwrap_or(false)
    })
}

fn file_matches(path: &Path, artifact: &ModelArtifact) -> Result<bool, ModelInstallError> {
    if file_size(path)? != artifact.bytes {
        return Ok(false);
    }
    Ok(sha256(path)? == artifact.sha256)
}

fn sha256(path: &Path) -> Result<String, ModelInstallError> {
    let mut file = File::open(path).map_err(filesystem_error)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(filesystem_error)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn file_size(path: &Path) -> Result<u64, ModelInstallError> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(metadata.len()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(filesystem_error(error)),
    }
}

fn remove_file_if_present(path: &Path) -> Result<(), ModelInstallError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(filesystem_error(error)),
    }
}

fn remove_directory_if_present(path: &Path) -> Result<(), ModelInstallError> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(filesystem_error(error)),
    }
}

fn download_error(error: impl std::fmt::Display) -> ModelInstallError {
    ModelInstallError::Download(error.to_string())
}

fn filesystem_error(error: impl std::fmt::Display) -> ModelInstallError {
    ModelInstallError::Filesystem(error.to_string())
}
