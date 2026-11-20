use super::*;
use futures_util::future::try_join_all;
use reqwest::header::{ACCEPT_RANGES, CONTENT_LENGTH, CONTENT_RANGE};
use std::{
    fs::File,
    io::Read,
    sync::atomic::{AtomicU64, Ordering},
};

const PARALLEL_ARTIFACT_THRESHOLD: u64 = 64 * 1024 * 1024;
const MAX_PARALLEL_RANGES: u64 = 4;
const RANGE_CHUNK_BYTES: u64 = 64 * 1024 * 1024;

struct RangeProgress {
    received: Arc<AtomicU64>,
    base_downloaded: u64,
    on_progress: Arc<dyn Fn(ModelDownloadProgress) + Send + Sync>,
}

impl VerifiedModelInstaller {
    pub(super) async fn download_artifact(
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

        if existing == 0
            && artifact.bytes >= PARALLEL_ARTIFACT_THRESHOLD
            && self.supports_parallel_ranges(artifact).await
        {
            return self
                .download_artifact_in_ranges(staging, artifact, &partial, on_progress, control)
                .await;
        }

        let request = self.artifact_request(artifact, existing);
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

    async fn supports_parallel_ranges(&self, artifact: &ModelArtifact) -> bool {
        let url = if artifact.remote_path.starts_with("https://") {
            artifact.remote_path.clone()
        } else {
            format!("{}/{}", self.manifest.base_url, artifact.remote_path)
        };
        let Ok(response) = self.client.head(url).send().await else {
            return false;
        };
        response.status().is_success()
            && response
                .headers()
                .get(ACCEPT_RANGES)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.split(',').any(|item| item.trim() == "bytes"))
            && response
                .headers()
                .get(CONTENT_LENGTH)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
                == Some(artifact.bytes)
    }

    async fn download_artifact_in_ranges(
        &self,
        staging: &Path,
        artifact: &ModelArtifact,
        partial: &Path,
        on_progress: Arc<dyn Fn(ModelDownloadProgress) + Send + Sync>,
        control: &ModelInstallControl,
    ) -> Result<(), ModelInstallError> {
        let range_count = (artifact.bytes.div_ceil(RANGE_CHUNK_BYTES)).min(MAX_PARALLEL_RANGES);
        let progress = RangeProgress {
            received: Arc::new(AtomicU64::new(0)),
            base_downloaded: staged_bytes(staging, &self.manifest)?,
            on_progress: Arc::clone(&on_progress),
        };
        let tasks = (0..range_count).map(|index| {
            let start = artifact.bytes * index / range_count;
            let end = if index + 1 == range_count {
                artifact.bytes - 1
            } else {
                artifact.bytes * (index + 1) / range_count - 1
            };
            self.download_range(
                artifact,
                partial.with_extension(format!("segment-{index}.part")),
                start,
                end,
                &progress,
                control,
            )
        });
        if let Err(error) = try_join_all(tasks).await {
            for index in 0..range_count {
                let _ = remove_file_if_present(
                    &partial.with_extension(format!("segment-{index}.part")),
                );
            }
            return Err(error);
        }

        let mut output = open_partial(partial, false)?;
        let mut buffer = [0_u8; 64 * 1024];
        for index in 0..range_count {
            let segment = partial.with_extension(format!("segment-{index}.part"));
            let mut input = File::open(&segment).map_err(filesystem_error)?;
            loop {
                control.check()?;
                let read = input.read(&mut buffer).map_err(filesystem_error)?;
                if read == 0 {
                    break;
                }
                output
                    .write_all(&buffer[..read])
                    .map_err(filesystem_error)?;
            }
            remove_file_if_present(&segment)?;
        }
        output.sync_all().map_err(filesystem_error)?;
        drop(output);
        self.finish_artifact(partial, &staging.join(&artifact.local_path), artifact)?;
        self.report_progress(staging, &on_progress)
    }

    async fn download_range(
        &self,
        artifact: &ModelArtifact,
        path: PathBuf,
        start: u64,
        end: u64,
        progress: &RangeProgress,
        control: &ModelInstallControl,
    ) -> Result<(), ModelInstallError> {
        let response = tokio::select! {
            () = control.cancellation.cancelled() => return control.check(),
            response = self.artifact_request(artifact, 0)
                .header(RANGE, format!("bytes={start}-{end}"))
                .send() => response.map_err(download_error)?,
        };
        let content_range = response
            .headers()
            .get(CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| {
                ModelInstallError::Download("range response omitted Content-Range".to_owned())
            })?;
        if response.status() != StatusCode::PARTIAL_CONTENT
            || content_range != format!("bytes {start}-{end}/{}", artifact.bytes)
        {
            return Err(ModelInstallError::Download(
                "server returned an invalid byte range".to_owned(),
            ));
        }
        let mut file = File::create(&path).map_err(filesystem_error)?;
        let expected = end - start + 1;
        let mut written = 0_u64;
        let mut stream = response.bytes_stream();
        while let Some(chunk) = tokio::select! {
            () = control.cancellation.cancelled() => return control.check(),
            chunk = stream.next() => chunk,
        } {
            let chunk = chunk.map_err(download_error)?;
            written = written.saturating_add(chunk.len() as u64);
            if written > expected {
                return Err(ModelInstallError::Integrity(
                    "range exceeded its fixed size".to_owned(),
                ));
            }
            file.write_all(&chunk).map_err(filesystem_error)?;
            let total = progress
                .received
                .fetch_add(chunk.len() as u64, Ordering::AcqRel)
                + chunk.len() as u64;
            (progress.on_progress)(ModelDownloadProgress {
                downloaded_bytes: progress.base_downloaded.saturating_add(total),
                total_bytes: self.manifest.total_bytes(),
            });
        }
        if written != expected {
            return Err(ModelInstallError::Download(format!(
                "range ended at {written} of {expected} bytes"
            )));
        }
        file.sync_all().map_err(filesystem_error)
    }

    pub(super) fn artifact_request(
        &self,
        artifact: &ModelArtifact,
        existing: u64,
    ) -> reqwest::RequestBuilder {
        let url = if artifact.remote_path.starts_with("https://") {
            artifact.remote_path.clone()
        } else {
            format!("{}/{}", self.manifest.base_url, artifact.remote_path)
        };
        let mut request = self.client.get(url);
        if existing > 0 {
            request = request.header(RANGE, format!("bytes={existing}-"));
        }
        request
    }

    pub(super) fn finish_artifact(
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

    pub(super) fn report_progress(
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
}
