use rusqlite::{Connection, OptionalExtension, params};
use template_app::{HistoryRetention, LocalSettings, StorageError, UiLanguagePreference};

use super::unavailable;

pub(super) fn load(connection: &Connection) -> Result<LocalSettings, StorageError> {
    connection
        .query_row(
            "SELECT history_enabled, history_retention_days,
                    preferred_microphone_id, preferred_microphone_name,
                    diagnostics_logging_enabled, ui_language,
                    automatic_update_checks, feedback_sounds_enabled
             FROM app_settings WHERE singleton = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, bool>(0)?,
                    row.get::<_, Option<u16>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, bool>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, bool>(6)?,
                    row.get::<_, bool>(7)?,
                ))
            },
        )
        .optional()
        .map_err(unavailable)?
        .ok_or_else(|| StorageError::Invalid("app settings row is missing".to_owned()))
        .and_then(
            |(
                history_enabled,
                retention_days,
                microphone_id,
                microphone_name,
                diagnostics_logging_enabled,
                ui_language,
                automatic_update_checks,
                feedback_sounds_enabled,
            )| {
                Ok(LocalSettings {
                    history_enabled,
                    history_retention: retention_from_days(retention_days)?,
                    preferred_microphone_id: microphone_id,
                    preferred_microphone_name: microphone_name,
                    diagnostics_logging_enabled,
                    ui_language: UiLanguagePreference::from_storage_value(&ui_language)
                        .ok_or_else(|| {
                            StorageError::Invalid(format!(
                                "unsupported UI language preference: {ui_language}"
                            ))
                        })?,
                    automatic_update_checks,
                    feedback_sounds_enabled,
                })
            },
        )
}

pub(super) fn save(
    connection: &mut Connection,
    settings: &LocalSettings,
) -> Result<(), StorageError> {
    connection
        .execute(
            "UPDATE app_settings SET
                history_enabled = ?1,
                history_retention_days = ?2,
                preferred_microphone_id = ?3,
                preferred_microphone_name = ?4,
                diagnostics_logging_enabled = ?5,
                ui_language = ?6,
                automatic_update_checks = ?7,
                feedback_sounds_enabled = ?8
             WHERE singleton = 1",
            params![
                settings.history_enabled,
                settings.history_retention.days(),
                settings.preferred_microphone_id,
                settings.preferred_microphone_name,
                settings.diagnostics_logging_enabled,
                settings.ui_language.storage_value(),
                settings.automatic_update_checks,
                settings.feedback_sounds_enabled,
            ],
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
