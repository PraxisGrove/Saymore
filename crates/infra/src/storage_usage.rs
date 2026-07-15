use std::{fs, io, path::Path};

use template_app::StorageError;

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

    use super::directory_usage_bytes;

    #[test]
    fn measures_regular_files_recursively() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        fs::write(directory.path().join("saymore.sqlite3"), [0_u8; 7])?;
        fs::create_dir(directory.path().join("logs"))?;
        fs::write(directory.path().join("logs/runtime.log"), [0_u8; 11])?;

        assert_eq!(18, directory_usage_bytes(directory.path())?);
        Ok(())
    }

    #[test]
    fn treats_a_missing_directory_as_empty() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let missing = directory.path().join("missing");

        assert_eq!(0, directory_usage_bytes(&missing)?);
        Ok(())
    }
}
