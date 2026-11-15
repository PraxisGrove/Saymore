use super::*;

impl VerifiedModelInstaller {
    /// Copies only pinned artifacts from a local directory and activates them after verification.
    /// The source is never modified; downloading and importing never select an ASR provider.
    pub fn import_directory(&self, source: &Path) -> Result<PathBuf, ModelInstallError> {
        if !matches!(self.manifest.preparation, ModelPreparation::Direct) {
            return Err(ModelInstallError::Activation(
                "Archive models must use the download installer".to_owned(),
            ));
        }
        if self.is_installed() {
            return Ok(self.model_directory());
        }
        for artifact in &self.manifest.artifacts {
            let path = source.join(&artifact.local_path);
            let metadata = fs::symlink_metadata(&path).map_err(filesystem_error)?;
            if !metadata.is_file() || !file_matches(&path, artifact)? {
                return Err(ModelInstallError::Integrity(format!(
                    "{} does not match the supported model export",
                    artifact.local_path
                )));
            }
        }
        fs::create_dir_all(&self.models_root).map_err(filesystem_error)?;
        // Import copies a complete artifact set even when a download is partially staged.
        let required = self
            .manifest
            .installed_bytes()
            .saturating_add(INSTALL_SPACE_MARGIN_BYTES);
        let available = fs2::available_space(&self.models_root).map_err(filesystem_error)?;
        if available < required {
            return Err(ModelInstallError::InsufficientSpace {
                required_bytes: required,
                available_bytes: available,
            });
        }
        let prepared = self.prepared_directory();
        remove_directory_if_present(&prepared)?;
        fs::create_dir_all(&prepared).map_err(filesystem_error)?;
        let result = (|| {
            for artifact in &self.manifest.artifacts {
                let destination = prepared.join(&artifact.local_path);
                if let Some(parent) = destination.parent() {
                    fs::create_dir_all(parent).map_err(filesystem_error)?;
                }
                fs::copy(source.join(&artifact.local_path), &destination)
                    .map_err(filesystem_error)?;
                fs::OpenOptions::new()
                    .write(true)
                    .open(&destination)
                    .and_then(|file| file.sync_all())
                    .map_err(filesystem_error)?;
            }
            if !manifest_matches(&prepared, &self.manifest) {
                return Err(ModelInstallError::Integrity(
                    "The imported model failed verification".to_owned(),
                ));
            }
            self.activate(&prepared)?;
            Ok(self.model_directory())
        })();
        if result.is_err() {
            let _ = remove_directory_if_present(&prepared);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use sha2::{Digest, Sha256};

    fn installer(root: &Path, contents: &[u8]) -> VerifiedModelInstaller {
        let manifest = ModelManifest::direct(
            "local-test",
            "v1",
            "",
            vec![artifact(
                "model.onnx",
                contents.len() as u64,
                &format!("{:x}", Sha256::digest(contents)),
            )],
        );

        VerifiedModelInstaller::new(root.to_owned(), manifest).unwrap()
    }

    #[test]
    fn import_checks_and_activates_only_pinned_files_without_modifying_source() {
        let root = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        fs::write(source.path().join("model.onnx"), b"verified model").unwrap();
        fs::write(source.path().join("unrelated.txt"), b"leave alone").unwrap();
        let installer = installer(root.path(), b"verified model");
        let target = installer.import_directory(source.path()).unwrap();
        assert!(installer.is_installed());
        assert_eq!(
            fs::read(target.join("model.onnx")).unwrap(),
            b"verified model"
        );
        assert!(!target.join("unrelated.txt").exists());
        assert_eq!(
            fs::read(source.path().join("unrelated.txt")).unwrap(),
            b"leave alone"
        );
    }

    #[tokio::test]
    #[allow(clippy::panic_in_result_fn)]
    async fn direct_download_and_local_import_share_verified_artifacts()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = httpmock::MockServer::start_async().await;
        let response = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET).path("/model.onnx");
                then.status(200).body("verified model");
            })
            .await;
        let download_root = tempfile::tempdir()?;
        let import_root = tempfile::tempdir()?;
        let mut downloader = installer(download_root.path(), b"verified model");
        downloader.manifest.base_url = server.base_url();
        let downloaded = downloader.install(Arc::new(|_| {})).await?;
        let imported = installer(import_root.path(), b"verified model");
        imported.import_directory(&downloaded)?;
        assert_eq!(
            imported.model_directory(),
            imported.install(Arc::new(|_| {})).await?
        );
        assert!(downloader.is_installed());
        assert!(imported.is_installed());
        response.assert_calls_async(1).await;
        Ok(())
    }

    #[test]
    fn invalid_import_does_not_activate_or_replace_existing_files() {
        let root = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        fs::write(source.path().join("model.onnx"), b"wrong").unwrap();
        let installer = installer(root.path(), b"right");
        fs::create_dir_all(installer.model_directory()).unwrap();
        fs::write(
            installer.model_directory().join("existing.txt"),
            b"preserve",
        )
        .unwrap();
        assert!(matches!(
            installer.import_directory(source.path()),
            Err(ModelInstallError::Integrity(_))
        ));
        assert!(!installer.is_installed());
        assert_eq!(
            fs::read(installer.model_directory().join("existing.txt")).unwrap(),
            b"preserve"
        );
    }

    #[test]
    #[ignore = "requires SAYMORE_SEACO_MODEL_DIR with the pinned quantized ONNX files"]
    fn imports_exact_seaco_export_and_loads_native_model() -> Result<(), Box<dyn std::error::Error>>
    {
        let source = PathBuf::from(std::env::var("SAYMORE_SEACO_MODEL_DIR")?);
        let root = tempfile::tempdir()?;
        let installer = VerifiedModelInstaller::seaco_paraformer(root.path().to_owned())?;
        let target = installer.import_directory(&source)?;
        if !installer.is_installed() {
            return Err("import did not activate the verified model".into());
        }
        let _recognizer = crate::SeacoSpeechRecognizer::load(&target)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn import_rejects_symbolic_links() {
        let root = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        fs::write(source.path().join("real"), b"model").unwrap();
        std::os::unix::fs::symlink(source.path().join("real"), source.path().join("model.onnx"))
            .unwrap();
        assert!(matches!(
            installer(root.path(), b"model").import_directory(source.path()),
            Err(ModelInstallError::Integrity(_))
        ));
    }
}
