use rusqlite::{Connection, params};
use template_app::{QueuedModelDownload, StorageError};

use super::unavailable;

pub(super) fn list(connection: &Connection) -> Result<Vec<QueuedModelDownload>, StorageError> {
    let mut statement = connection
        .prepare("SELECT model_id FROM model_download_queue ORDER BY sequence")
        .map_err(unavailable)?;
    statement
        .query_map([], |row| {
            Ok(QueuedModelDownload {
                model_id: row.get(0)?,
            })
        })
        .map_err(unavailable)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(unavailable)
}

pub(super) fn enqueue(connection: &Connection, model_id: &str) -> Result<(), StorageError> {
    validate_model_id(model_id)?;
    connection
        .execute(
            "INSERT OR IGNORE INTO model_download_queue(model_id) VALUES (?1)",
            params![model_id],
        )
        .map_err(unavailable)?;
    Ok(())
}

pub(super) fn remove(connection: &Connection, model_id: &str) -> Result<(), StorageError> {
    validate_model_id(model_id)?;
    connection
        .execute(
            "DELETE FROM model_download_queue WHERE model_id = ?1",
            params![model_id],
        )
        .map_err(unavailable)?;
    Ok(())
}

fn validate_model_id(model_id: &str) -> Result<(), StorageError> {
    if model_id.trim().is_empty() {
        return Err(StorageError::Invalid(
            "queued model identity is empty".to_owned(),
        ));
    }
    Ok(())
}
