use std::{fs, io, path::Path};

use template_app::{LocalModelStorageUsage, LocalStorageUsage, StorageError};

use crate::{
    PARAFORMER_MODEL_ID, PARAFORMER_MODEL_REVISION, PUNCTUATION_MODEL_ID,
    PUNCTUATION_MODEL_REVISION, QWEN3_ASR_MODEL_ID, QWEN3_ASR_MODEL_REVISION, SENSE_VOICE_MODEL_ID,
    SENSE_VOICE_MODEL_REVISION, WHISPER_MODEL_ID, WHISPER_MODEL_REVISION,
};

/// Returns the size of regular files managed beneath the application's data directory.
/// Symbolic links are intentionally not followed so a user-owned link cannot expand the
/// accounting scope outside of Saymore's own data directory.
pub fn directory_usage_bytes(directory: &Path) -> Result<u64, StorageError> {
    match fs::read_dir(directory) {
        Ok(mut entries) => entries.try_fold(0_u64, |total, entry| {
            let entry = entry.map_err(|error| unavailable(directory, error))?;
            let bytes = entry_usage_bytes(&entry.path())?;
            Ok(total.saturating_add(bytes))
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(unavailable(directory, error)),
    }
}

/// Measures Saymore-owned data without following symbolic links outside the data directory.
pub fn local_storage_usage(directory: &Path) -> Result<LocalStorageUsage, StorageError> {
    let models_directory = directory.join("models");
    let local_models_bytes = directory_usage_bytes(&models_directory)?;
    let recognition_data_bytes = sqlite_usage_bytes(&directory.join("saymore.sqlite3"))?;
    let diagnostic_logs_bytes = directory_usage_bytes(&directory.join("logs"))?;
    let total_bytes = directory_usage_bytes(directory)?;
    let classified_bytes = local_models_bytes
        .saturating_add(recognition_data_bytes)
        .saturating_add(diagnostic_logs_bytes);
    let models = LocalModelStorageUsage {
        paraformer_bytes: model_usage(
            &models_directory,
            PARAFORMER_MODEL_ID,
            PARAFORMER_MODEL_REVISION,
        )?,
        whisper_bytes: model_usage(&models_directory, WHISPER_MODEL_ID, WHISPER_MODEL_REVISION)?,
        qwen3_asr_bytes: model_usage(
            &models_directory,
            QWEN3_ASR_MODEL_ID,
            QWEN3_ASR_MODEL_REVISION,
        )?,
        sense_voice_bytes: model_usage(
            &models_directory,
            SENSE_VOICE_MODEL_ID,
            SENSE_VOICE_MODEL_REVISION,
        )?,
        punctuation_bytes: model_usage(
            &models_directory,
            PUNCTUATION_MODEL_ID,
            PUNCTUATION_MODEL_REVISION,
        )?,
    };
    Ok(LocalStorageUsage {
        total_bytes,
        local_models_bytes,
        recognition_data_bytes,
        diagnostic_logs_bytes,
        configuration_other_bytes: total_bytes.saturating_sub(classified_bytes),
        models,
    })
}

fn sqlite_usage_bytes(database: &Path) -> Result<u64, StorageError> {
    [
        database.to_path_buf(),
        database.with_extension("sqlite3-wal"),
        database.with_extension("sqlite3-shm"),
    ]
    .into_iter()
    .try_fold(0_u64, |total, path| {
        file_usage_bytes(&path).map(|bytes| total.saturating_add(bytes))
    })
}

fn model_usage(root: &Path, id: &str, revision: &str) -> Result<u64, StorageError> {
    directory_usage_bytes(&root.join(id).join(revision))
}

fn file_usage_bytes(path: &Path) -> Result<u64, StorageError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(metadata.len()),
        Ok(_) => Ok(0),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(unavailable(path, error)),
    }
}

fn entry_usage_bytes(path: &Path) -> Result<u64, StorageError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| unavailable(path, error))?;
    if metadata.is_file() {
        return Ok(metadata.len());
    }
    if metadata.is_dir() {
        return directory_usage_bytes(path);
    }
    Ok(0)
}

fn unavailable(path: &Path, error: io::Error) -> StorageError {
    StorageError::Unavailable(format!("cannot measure {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::{PARAFORMER_MODEL_ID, PARAFORMER_MODEL_REVISION};

    use super::{directory_usage_bytes, local_storage_usage};

    #[test]
    fn measures_regular_files_recursively() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        fs::write(directory.path().join("saymore.sqlite3"), [0_u8; 7])?;
        fs::create_dir(directory.path().join("logs"))?;
        fs::write(directory.path().join("logs/runtime.log"), [0_u8; 11])?;

        let actual = directory_usage_bytes(directory.path())?;
        if actual != 18 {
            return Err(format!("expected 18 bytes, measured {actual}").into());
        }
        Ok(())
    }

    #[test]
    fn treats_a_missing_directory_as_empty() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let missing = directory.path().join("missing");

        let actual = directory_usage_bytes(&missing)?;
        if actual != 0 {
            return Err(format!("expected 0 bytes, measured {actual}").into());
        }
        Ok(())
    }

    #[test]
    fn classifies_models_database_logs_and_other_files() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let model = directory
            .path()
            .join("models")
            .join(PARAFORMER_MODEL_ID)
            .join(PARAFORMER_MODEL_REVISION);
        fs::create_dir_all(&model)?;
        fs::write(model.join("encoder.onnx"), [0_u8; 11])?;
        fs::write(directory.path().join("saymore.sqlite3"), [0_u8; 7])?;
        fs::write(directory.path().join("saymore.sqlite3-wal"), [0_u8; 5])?;
        fs::create_dir(directory.path().join("logs"))?;
        fs::write(directory.path().join("logs/runtime.log"), [0_u8; 3])?;
        fs::write(directory.path().join("config.json"), [0_u8; 2])?;

        let usage = local_storage_usage(directory.path())?;

        let actual = [
            usage.total_bytes,
            usage.local_models_bytes,
            usage.recognition_data_bytes,
            usage.diagnostic_logs_bytes,
            usage.configuration_other_bytes,
            usage.models.paraformer_bytes,
        ];
        let expected = [28, 11, 12, 3, 2, 11];
        if actual != expected {
            return Err(format!("unexpected storage classification: {actual:?}").into());
        }
        Ok(())
    }
}
