use std::{
    fs::{self, File, OpenOptions},
    io::Read,
    path::Path,
};

use bzip2::read::BzDecoder;
use sha2::{Digest, Sha256};

use super::{ModelArtifact, ModelInstallError, ModelManifest};

pub(super) fn extract_tar_bzip2_member(
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

pub(super) fn open_partial(path: &Path, append: bool) -> Result<File, ModelInstallError> {
    OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(path)
        .map_err(filesystem_error)
}

pub(super) fn staged_bytes(
    staging: &Path,
    manifest: &ModelManifest,
) -> Result<u64, ModelInstallError> {
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

pub(super) fn manifest_matches(directory: &Path, manifest: &ModelManifest) -> bool {
    manifest.artifacts.iter().all(|artifact| {
        file_matches(&directory.join(&artifact.local_path), artifact).unwrap_or(false)
    })
}

pub(super) fn file_matches(
    path: &Path,
    artifact: &ModelArtifact,
) -> Result<bool, ModelInstallError> {
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

pub(super) fn file_size(path: &Path) -> Result<u64, ModelInstallError> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(metadata.len()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(filesystem_error(error)),
    }
}

pub(super) fn remove_file_if_present(path: &Path) -> Result<(), ModelInstallError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(filesystem_error(error)),
    }
}

pub(super) fn remove_directory_if_present(path: &Path) -> Result<(), ModelInstallError> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(filesystem_error(error)),
    }
}

pub(super) fn download_error(error: impl std::fmt::Display) -> ModelInstallError {
    ModelInstallError::Download(error.to_string())
}

pub(super) fn filesystem_error(error: impl std::fmt::Display) -> ModelInstallError {
    ModelInstallError::Filesystem(error.to_string())
}
