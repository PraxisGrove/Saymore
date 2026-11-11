use std::{
    fs, io,
    path::{Path, PathBuf},
};

use template_app::StorageError;

use crate::{
    PARAFORMER_MODEL_ID, PARAFORMER_MODEL_REVISION, PUNCTUATION_MODEL_ID,
    PUNCTUATION_MODEL_REVISION, directory_usage_bytes,
};

/// Retired model files remain user-managed; these identities never enable inference.
#[derive(Clone, Copy)]
pub enum RetiredAsrModel {
    Paraformer,
    Punctuation,
}

impl RetiredAsrModel {
    pub fn id(self) -> &'static str {
        match self {
            Self::Paraformer => PARAFORMER_MODEL_ID,
            Self::Punctuation => PUNCTUATION_MODEL_ID,
        }
    }

    fn revision(self) -> &'static str {
        match self {
            Self::Paraformer => PARAFORMER_MODEL_REVISION,
            Self::Punctuation => PUNCTUATION_MODEL_REVISION,
        }
    }

    fn directories(self, root: &Path) -> [PathBuf; 4] {
        let id = self.id();
        let revision = self.revision();
        [
            root.join(id),
            root.join(format!(".{id}-{revision}.download")),
            root.join(format!(".{id}-{revision}.install")),
            root.join(format!(".{id}-{revision}.backup")),
        ]
    }

    /// Includes old revisions and interrupted downloads without following directory links.
    pub fn storage_bytes(self, root: &Path) -> Result<u64, StorageError> {
        self.directories(root)
            .iter()
            .try_fold(0_u64, |total, path| {
                let metadata = match fs::symlink_metadata(path) {
                    Ok(metadata) => metadata,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(total),
                    Err(_) => {
                        return Err(StorageError::Unavailable(
                            "could not inspect retired model files".to_owned(),
                        ));
                    }
                };
                if metadata.is_dir() {
                    directory_usage_bytes(path).map(|bytes| total.saturating_add(bytes))
                } else {
                    Ok(total)
                }
            })
    }

    /// Removes only this retired model's managed directories after an explicit user action.
    pub fn remove_files(self, root: &Path) -> Result<(), io::Error> {
        for path in self.directories(root) {
            match fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path)?,
                Ok(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "retired model path is not a regular directory",
                    ));
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn explicit_cleanup_handles_corrupt_partial_files_and_preserves_other_models() {
        let root = tempfile::tempdir().unwrap();
        for model in [RetiredAsrModel::Paraformer, RetiredAsrModel::Punctuation] {
            for path in model.directories(root.path()) {
                fs::create_dir_all(&path).unwrap();
                fs::write(path.join("partial"), b"unverified").unwrap();
            }
            assert_eq!(40, model.storage_bytes(root.path()).unwrap());
        }
        let current = root.path().join(crate::SEACO_MODEL_ID);
        fs::create_dir_all(&current).unwrap();
        fs::write(current.join("model.onnx"), b"preserve").unwrap();
        RetiredAsrModel::Paraformer
            .remove_files(root.path())
            .unwrap();
        assert_eq!(
            0,
            RetiredAsrModel::Paraformer
                .storage_bytes(root.path())
                .unwrap()
        );
        assert_eq!(
            40,
            RetiredAsrModel::Punctuation
                .storage_bytes(root.path())
                .unwrap()
        );
        RetiredAsrModel::Punctuation
            .remove_files(root.path())
            .unwrap();
        assert_eq!(
            b"preserve",
            fs::read(current.join("model.onnx")).unwrap().as_slice()
        );
    }

    #[cfg(unix)]
    #[test]
    fn cleanup_refuses_linked_roots_and_never_counts_external_files() {
        let root = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        fs::write(external.path().join("keep"), b"private").unwrap();
        std::os::unix::fs::symlink(external.path(), root.path().join(PARAFORMER_MODEL_ID)).unwrap();
        assert_eq!(
            0,
            RetiredAsrModel::Paraformer
                .storage_bytes(root.path())
                .unwrap()
        );
        assert!(
            RetiredAsrModel::Paraformer
                .remove_files(root.path())
                .is_err()
        );
        assert_eq!(
            b"private",
            fs::read(external.path().join("keep")).unwrap().as_slice()
        );
    }
}
