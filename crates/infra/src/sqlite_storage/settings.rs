use rusqlite::{Connection, OptionalExtension, params};
use template_app::{HistoryRetention, LocalSettings, StorageError};

use super::unavailable;

pub(super) fn load(connection: &Connection) -> Result<LocalSettings, StorageError> {
    connection
        .query_row(
            "SELECT history_enabled, history_retention_days
             FROM app_settings WHERE singleton = 1",
            [],
            |row| Ok((row.get::<_, bool>(0)?, row.get::<_, Option<u16>>(1)?)),
        )
        .optional()
        .map_err(unavailable)?
        .ok_or_else(|| StorageError::Invalid("app settings row is missing".to_owned()))
        .and_then(|(history_enabled, retention_days)| {
            Ok(LocalSettings {
                history_enabled,
                history_retention: retention_from_days(retention_days)?,
            })
        })
}

pub(super) fn save(
    connection: &mut Connection,
    settings: &LocalSettings,
) -> Result<(), StorageError> {
    connection
        .execute(
            "UPDATE app_settings SET
                history_enabled = ?1,
                history_retention_days = ?2
             WHERE singleton = 1",
            params![settings.history_enabled, settings.history_retention.days()],
        )
        .map_err(unavailable)?;
    Ok(())
}

fn retention_from_days(days: Option<u16>) -> Result<HistoryRetention, StorageError> {
    match days {
        Some(1) => Ok(HistoryRetention::OneDay),
        Some(7) => Ok(HistoryRetention::SevenDays),
        Some(30) => Ok(HistoryRetention::ThirtyDays),
        None => Ok(HistoryRetention::Forever),
        Some(other) => Err(StorageError::Invalid(format!(
            "unsupported history retention: {other} days"
        ))),
    }
}
