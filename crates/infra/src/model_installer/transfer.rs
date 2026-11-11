use super::*;

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
