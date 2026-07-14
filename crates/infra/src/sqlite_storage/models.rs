use rusqlite::{Connection, params};
use template_app::{InstalledModel, StorageError};

use super::unavailable;

pub(super) fn list(connection: &Connection) -> Result<Vec<InstalledModel>, StorageError> {
    let mut statement = connection
        .prepare(
            "SELECT id, provider_type, model, version, path, installed_at_ms, last_verified_at_ms
             FROM installed_models ORDER BY installed_at_ms DESC, id",
        )
        .map_err(unavailable)?;
    statement
        .query_map([], |row| {
            Ok(InstalledModel {
                id: row.get(0)?,
                provider_type: row.get(1)?,
                model: row.get(2)?,
                version: row.get(3)?,
                path: row.get(4)?,
                installed_at_ms: row.get(5)?,
                last_verified_at_ms: row.get(6)?,
            })
        })
        .map_err(unavailable)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(unavailable)
}

pub(super) fn save(connection: &mut Connection, model: InstalledModel) -> Result<(), StorageError> {
    if model.id.trim().is_empty()
        || model.provider_type.trim().is_empty()
        || model.model.trim().is_empty()
        || model.version.trim().is_empty()
        || model.path.trim().is_empty()
    {
        return Err(StorageError::Invalid(
            "installed model metadata contains an empty required field".to_owned(),
        ));
    }
    connection
        .execute(
            "INSERT INTO installed_models(
                id, provider_type, model, version, path, installed_at_ms, last_verified_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET
                provider_type = excluded.provider_type,
                model = excluded.model,
                version = excluded.version,
                path = excluded.path,
                installed_at_ms = excluded.installed_at_ms,
                last_verified_at_ms = excluded.last_verified_at_ms",
            params![
                model.id,
                model.provider_type,
                model.model,
                model.version,
                model.path,
                model.installed_at_ms,
                model.last_verified_at_ms
            ],
        )
        .map_err(unavailable)?;
    Ok(())
}
