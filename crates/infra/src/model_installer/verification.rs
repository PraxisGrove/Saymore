use std::{fs, path::Path, time::UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::{ModelInstallError, ModelManifest, filesystem_error};

pub(super) const VERIFICATION_MARKER: &str = ".saymore-verified.json";

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
struct VerificationMarker {
    model_id: String,
    revision: String,
    artifacts: Vec<VerifiedArtifact>,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
struct VerifiedArtifact {
    local_path: String,
    bytes: u64,
    sha256: String,
    modified_seconds: u64,
    modified_nanoseconds: u32,
}

pub(super) fn verification_marker_matches(directory: &Path, manifest: &ModelManifest) -> bool {
    let marker = fs::read(directory.join(VERIFICATION_MARKER))
        .ok()
        .and_then(|contents| serde_json::from_slice::<VerificationMarker>(&contents).ok());
    marker
        .zip(verification_marker(directory, manifest).ok())
        .is_some_and(|(stored, current)| stored == current)
}

pub(super) fn write_verification_marker(
    directory: &Path,
    manifest: &ModelManifest,
) -> Result<(), ModelInstallError> {
    let marker = verification_marker(directory, manifest)?;
    let contents = serde_json::to_vec(&marker).map_err(filesystem_error)?;
    fs::write(directory.join(VERIFICATION_MARKER), contents).map_err(filesystem_error)
}

fn verification_marker(
    directory: &Path,
    manifest: &ModelManifest,
) -> Result<VerificationMarker, ModelInstallError> {
    let artifacts = manifest
        .artifacts
        .iter()
        .map(|artifact| {
            let metadata =
                fs::metadata(directory.join(&artifact.local_path)).map_err(filesystem_error)?;
            let modified = metadata
                .modified()
                .map_err(filesystem_error)?
                .duration_since(UNIX_EPOCH)
                .map_err(filesystem_error)?;
            Ok(VerifiedArtifact {
                local_path: artifact.local_path.clone(),
                bytes: metadata.len(),
                sha256: artifact.sha256.clone(),
                modified_seconds: modified.as_secs(),
                modified_nanoseconds: modified.subsec_nanos(),
            })
        })
        .collect::<Result<Vec<_>, ModelInstallError>>()?;
    Ok(VerificationMarker {
        model_id: manifest.id.clone(),
        revision: manifest.revision.clone(),
        artifacts,
    })
}
