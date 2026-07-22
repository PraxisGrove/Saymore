use rusqlite::Connection;
use template_app::StorageError;

use super::{app_settings_has_column, unavailable};

pub(super) fn add_dictionary_candidate_evidence(
    connection: &mut Connection,
) -> Result<(), StorageError> {
    let transaction = connection.transaction().map_err(unavailable)?;
    transaction
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS dictionary_candidate_evidence (
                language TEXT NOT NULL,
                canonical_key TEXT NOT NULL,
                canonical TEXT NOT NULL,
                decision TEXT NOT NULL,
                candidate_kind TEXT NOT NULL,
                confidence INTEGER NOT NULL CHECK (confidence BETWEEN 0 AND 100),
                assessment_source TEXT NOT NULL,
                occurrence_count INTEGER NOT NULL,
                dictation_count INTEGER NOT NULL,
                state TEXT NOT NULL CHECK (state IN ('pending', 'promoted')),
                last_observed_at_ms INTEGER NOT NULL,
                PRIMARY KEY(language, canonical_key)
             );
             PRAGMA user_version = 14;",
        )
        .map_err(unavailable)?;
    transaction.commit().map_err(unavailable)
}

pub(super) fn add_appearance_settings(connection: &mut Connection) -> Result<(), StorageError> {
    let transaction = connection.transaction().map_err(unavailable)?;
    if !app_settings_has_column(&transaction, "theme_id")? {
        transaction
            .execute_batch(
                "ALTER TABLE app_settings
                 ADD COLUMN theme_id TEXT NOT NULL DEFAULT 'lime-pulse'
                 CHECK (theme_id IN (
                    'warm-clay', 'lime-pulse', 'berry-graphite', 'iris-mist', 'clear-sky'
                 ));
                 ALTER TABLE app_settings
                 ADD COLUMN color_scheme TEXT NOT NULL DEFAULT 'system'
                 CHECK (color_scheme IN ('system', 'light', 'dark'));",
            )
            .map_err(unavailable)?;
    }
    transaction
        .execute_batch("PRAGMA user_version = 15;")
        .map_err(unavailable)?;
    transaction.commit().map_err(unavailable)
}

pub(super) fn add_system_audio_mute_setting(
    connection: &mut Connection,
) -> Result<(), StorageError> {
    let transaction = connection.transaction().map_err(unavailable)?;
    if !app_settings_has_column(&transaction, "mute_system_audio_enabled")? {
        transaction
            .execute_batch(
                "ALTER TABLE app_settings
                 ADD COLUMN mute_system_audio_enabled INTEGER NOT NULL DEFAULT 1
                 CHECK (mute_system_audio_enabled IN (0, 1));",
            )
            .map_err(unavailable)?;
    }
    transaction
        .execute_batch("PRAGMA user_version = 16;")
        .map_err(unavailable)?;
    transaction.commit().map_err(unavailable)
}

pub(super) fn add_diagnostic_events(connection: &mut Connection) -> Result<(), StorageError> {
    let transaction = connection.transaction().map_err(unavailable)?;
    transaction
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS diagnostic_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                recorded_at_ms INTEGER NOT NULL,
                event TEXT NOT NULL CHECK (length(event) BETWEEN 1 AND 120)
             );
             CREATE INDEX IF NOT EXISTS diagnostic_events_order
                ON diagnostic_events(id DESC);
             PRAGMA user_version = 17;",
        )
        .map_err(unavailable)?;
    transaction.commit().map_err(unavailable)
}
