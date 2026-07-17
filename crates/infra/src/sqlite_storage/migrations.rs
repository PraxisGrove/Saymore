use rusqlite::{Connection, Transaction};
use template_app::{StorageError, dictionary_comparison_key};

use super::unavailable;

const CURRENT_SCHEMA_VERSION: u32 = 14;

pub(super) fn apply(connection: &mut Connection) -> Result<(), StorageError> {
    let version: u32 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(unavailable)?;
    let fresh_install = version == 0;
    if version > CURRENT_SCHEMA_VERSION {
        return Err(StorageError::NewerSchema(version));
    }
    if version == 0 {
        migrate_settings(connection)?;
    }
    if version < 2 {
        migrate_history(connection)?;
    }
    if version < 3 {
        migrate_dictionary_and_models(connection)?;
    }
    if version < 4 {
        preserve_dictionary_token_boundaries(connection)?;
    }
    if version < 5 {
        remove_dictionary_variant_mappings(connection)?;
    }
    if version < 6 {
        add_preferred_microphone(connection)?;
    }
    if version < 7 {
        add_diagnostics_logging_setting(connection)?;
    }
    if version < 8 {
        add_ui_language_setting(connection)?;
    }
    if version < 9 {
        add_runtime_preference_settings(connection)?;
    }
    if version < 10 {
        add_desktop_behavior_settings(connection)?;
    }
    if version < 11 {
        add_dictation_shortcut_setting(connection)?;
    }
    if version < 12 {
        add_multiple_dictation_shortcuts(connection, fresh_install)?;
    }
    if version < 13 {
        add_onboarding_state(connection, fresh_install)?;
    }
    if version < 14 {
        add_dictionary_candidate_evidence(connection)?;
    }
    Ok(())
}

fn add_dictionary_candidate_evidence(connection: &mut Connection) -> Result<(), StorageError> {
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

fn add_onboarding_state(
    connection: &mut Connection,
    fresh_install: bool,
) -> Result<(), StorageError> {
    let transaction = connection.transaction().map_err(unavailable)?;
    if !app_settings_has_column(&transaction, "onboarding_status")? {
        transaction
            .execute_batch(
                "ALTER TABLE app_settings
                 ADD COLUMN onboarding_status TEXT NOT NULL DEFAULT 'not_started'
                 CHECK (onboarding_status IN (
                    'not_started', 'in_progress', 'completed', 'skipped'
                 ));
                 ALTER TABLE app_settings
                 ADD COLUMN onboarding_step INTEGER NOT NULL DEFAULT 0
                 CHECK (onboarding_step BETWEEN 0 AND 3);",
            )
            .map_err(unavailable)?;
        if !fresh_install {
            transaction
                .execute(
                    "UPDATE app_settings SET onboarding_status = 'completed', onboarding_step = 3",
                    [],
                )
                .map_err(unavailable)?;
        }
    }
    transaction
        .execute_batch("PRAGMA user_version = 13;")
        .map_err(unavailable)?;
    transaction.commit().map_err(unavailable)
}

fn add_multiple_dictation_shortcuts(
    connection: &mut Connection,
    fresh_install: bool,
) -> Result<(), StorageError> {
    let transaction = connection.transaction().map_err(unavailable)?;
    if !app_settings_has_column(&transaction, "dictation_shortcuts")? {
        transaction
            .execute_batch(
                "ALTER TABLE app_settings
                 ADD COLUMN dictation_shortcuts TEXT NOT NULL DEFAULT 'right-command';
                 UPDATE app_settings
                 SET dictation_shortcuts = dictation_shortcut
                 WHERE dictation_shortcut <> '';",
            )
            .map_err(unavailable)?;
    }
    if fresh_install && let Some(shortcut) = fresh_install_shortcut() {
        transaction
            .execute(
                "UPDATE app_settings SET dictation_shortcuts = ?1 WHERE singleton = 1",
                [shortcut],
            )
            .map_err(unavailable)?;
    }
    transaction
        .execute_batch("PRAGMA user_version = 12;")
        .map_err(unavailable)?;
    transaction.commit().map_err(unavailable)
}

#[cfg(target_os = "windows")]
fn fresh_install_shortcut() -> Option<&'static str> {
    Some("windows:right-alt")
}

#[cfg(not(target_os = "windows"))]
fn fresh_install_shortcut() -> Option<&'static str> {
    None
}

fn add_dictation_shortcut_setting(connection: &mut Connection) -> Result<(), StorageError> {
    let transaction = connection.transaction().map_err(unavailable)?;
    if !app_settings_has_column(&transaction, "dictation_shortcut")? {
        transaction
            .execute_batch(
                "ALTER TABLE app_settings
                 ADD COLUMN dictation_shortcut TEXT NOT NULL DEFAULT 'right-command';",
            )
            .map_err(unavailable)?;
    }
    transaction
        .execute_batch("PRAGMA user_version = 11;")
        .map_err(unavailable)?;
    transaction.commit().map_err(unavailable)
}

fn add_desktop_behavior_settings(connection: &mut Connection) -> Result<(), StorageError> {
    let transaction = connection.transaction().map_err(unavailable)?;
    if !app_settings_has_column(&transaction, "copy_to_clipboard")? {
        transaction
            .execute_batch(
                "ALTER TABLE app_settings
                 ADD COLUMN copy_to_clipboard INTEGER NOT NULL DEFAULT 0
                 CHECK (copy_to_clipboard IN (0, 1));",
            )
            .map_err(unavailable)?;
    }
    if !app_settings_has_column(&transaction, "show_in_dock")? {
        transaction
            .execute_batch(
                "ALTER TABLE app_settings
                 ADD COLUMN show_in_dock INTEGER NOT NULL DEFAULT 1
                 CHECK (show_in_dock IN (0, 1));",
            )
            .map_err(unavailable)?;
    }
    if !app_settings_has_column(&transaction, "dictation_paused")? {
        transaction
            .execute_batch(
                "ALTER TABLE app_settings
                 ADD COLUMN dictation_paused INTEGER NOT NULL DEFAULT 0
                 CHECK (dictation_paused IN (0, 1));",
            )
            .map_err(unavailable)?;
    }
    transaction
        .execute_batch("PRAGMA user_version = 10;")
        .map_err(unavailable)?;
    transaction.commit().map_err(unavailable)
}

fn add_runtime_preference_settings(connection: &mut Connection) -> Result<(), StorageError> {
    let transaction = connection.transaction().map_err(unavailable)?;
    if !app_settings_has_column(&transaction, "automatic_update_checks")? {
        transaction
            .execute_batch(
                "ALTER TABLE app_settings
                 ADD COLUMN automatic_update_checks INTEGER NOT NULL DEFAULT 0
                 CHECK (automatic_update_checks IN (0, 1));",
            )
            .map_err(unavailable)?;
    }
    if !app_settings_has_column(&transaction, "feedback_sounds_enabled")? {
        transaction
            .execute_batch(
                "ALTER TABLE app_settings
                 ADD COLUMN feedback_sounds_enabled INTEGER NOT NULL DEFAULT 1
                 CHECK (feedback_sounds_enabled IN (0, 1));",
            )
            .map_err(unavailable)?;
    }
    transaction
        .execute_batch("PRAGMA user_version = 9;")
        .map_err(unavailable)?;
    transaction.commit().map_err(unavailable)
}

fn add_ui_language_setting(connection: &mut Connection) -> Result<(), StorageError> {
    let transaction = connection.transaction().map_err(unavailable)?;
    if !app_settings_has_column(&transaction, "ui_language")? {
        transaction
            .execute_batch(
                "ALTER TABLE app_settings
                 ADD COLUMN ui_language TEXT NOT NULL DEFAULT 'system'
                 CHECK (ui_language IN ('system', 'en', 'zh-Hans'));",
            )
            .map_err(unavailable)?;
    }
    transaction
        .execute_batch("PRAGMA user_version = 8;")
        .map_err(unavailable)?;
    transaction.commit().map_err(unavailable)
}

fn add_preferred_microphone(connection: &mut Connection) -> Result<(), StorageError> {
    let transaction = connection.transaction().map_err(unavailable)?;
    if !app_settings_has_column(&transaction, "preferred_microphone_id")? {
        transaction
            .execute_batch("ALTER TABLE app_settings ADD COLUMN preferred_microphone_id TEXT;")
            .map_err(unavailable)?;
    }
    if !app_settings_has_column(&transaction, "preferred_microphone_name")? {
        transaction
            .execute_batch("ALTER TABLE app_settings ADD COLUMN preferred_microphone_name TEXT;")
            .map_err(unavailable)?;
    }
    transaction
        .execute_batch("PRAGMA user_version = 6;")
        .map_err(unavailable)?;
    transaction.commit().map_err(unavailable)
}

fn add_diagnostics_logging_setting(connection: &mut Connection) -> Result<(), StorageError> {
    let transaction = connection.transaction().map_err(unavailable)?;
    if !app_settings_has_column(&transaction, "diagnostics_logging_enabled")? {
        transaction
            .execute_batch(
                "ALTER TABLE app_settings
                 ADD COLUMN diagnostics_logging_enabled INTEGER NOT NULL DEFAULT 0
                 CHECK (diagnostics_logging_enabled IN (0, 1));",
            )
            .map_err(unavailable)?;
    }
    transaction
        .execute_batch("PRAGMA user_version = 7;")
        .map_err(unavailable)?;
    transaction.commit().map_err(unavailable)
}

fn app_settings_has_column(
    transaction: &Transaction<'_>,
    column: &str,
) -> Result<bool, StorageError> {
    transaction
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM pragma_table_info('app_settings') WHERE name = ?1
             )",
            [column],
            |row| row.get(0),
        )
        .map_err(unavailable)
}

fn remove_dictionary_variant_mappings(connection: &mut Connection) -> Result<(), StorageError> {
    let transaction = connection.transaction().map_err(unavailable)?;
    transaction
        .execute_batch(
            "ALTER TABLE term_observations RENAME TO term_observations_with_variants;
             ALTER TABLE dictionary_candidates RENAME TO dictionary_candidates_with_variants;
             DROP INDEX term_observations_candidate;
             CREATE TABLE term_observations (
                dictation_id TEXT NOT NULL,
                language TEXT NOT NULL,
                canonical TEXT NOT NULL,
                canonical_key TEXT NOT NULL,
                occurrence_count INTEGER NOT NULL CHECK (occurrence_count > 0),
                observed_at_ms INTEGER NOT NULL,
                PRIMARY KEY(dictation_id, language, canonical_key)
             );
             CREATE INDEX term_observations_candidate
                ON term_observations(language, canonical_key, observed_at_ms);
             INSERT INTO term_observations(
                dictation_id, language, canonical, canonical_key,
                occurrence_count, observed_at_ms
             )
             SELECT dictation_id, language, MAX(canonical), canonical_key,
                    SUM(occurrence_count), MAX(observed_at_ms)
             FROM term_observations_with_variants
             GROUP BY dictation_id, language, canonical_key;
             CREATE TABLE dictionary_candidates (
                language TEXT NOT NULL,
                canonical_key TEXT NOT NULL,
                occurrence_count INTEGER NOT NULL,
                dictation_count INTEGER NOT NULL,
                last_observed_at_ms INTEGER NOT NULL,
                PRIMARY KEY(language, canonical_key)
             );
             INSERT INTO dictionary_candidates(
                language, canonical_key, occurrence_count,
                dictation_count, last_observed_at_ms
             )
             SELECT language, canonical_key, SUM(occurrence_count),
                    COUNT(DISTINCT dictation_id), MAX(observed_at_ms)
             FROM term_observations
             GROUP BY language, canonical_key;
             DROP TABLE term_observations_with_variants;
             DROP TABLE dictionary_candidates_with_variants;
             DROP TABLE IF EXISTS dictionary_variants;
             PRAGMA user_version = 5;",
        )
        .map_err(unavailable)?;
    transaction.commit().map_err(unavailable)
}

fn preserve_dictionary_token_boundaries(connection: &mut Connection) -> Result<(), StorageError> {
    let transaction = connection.transaction().map_err(unavailable)?;
    let entries = {
        let mut statement = transaction
            .prepare("SELECT id, canonical FROM dictionary_entries")
            .map_err(unavailable)?;
        statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(unavailable)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(unavailable)?
    };
    for (id, canonical) in entries {
        transaction
            .execute(
                "UPDATE dictionary_entries SET canonical_key = ?1 WHERE id = ?2",
                (dictionary_comparison_key(&canonical), id),
            )
            .map_err(unavailable)?;
    }
    transaction
        .execute_batch("PRAGMA user_version = 4;")
        .map_err(unavailable)?;
    transaction.commit().map_err(unavailable)
}

fn migrate_dictionary_and_models(connection: &mut Connection) -> Result<(), StorageError> {
    let transaction = connection.transaction().map_err(unavailable)?;
    transaction
        .execute_batch(
            "CREATE TABLE trigger_bindings (
                id TEXT PRIMARY KEY NOT NULL,
                kind TEXT NOT NULL,
                binding TEXT NOT NULL,
                created_at_ms INTEGER NOT NULL
            );
            CREATE TABLE dictionary_entries (
                id TEXT PRIMARY KEY NOT NULL,
                canonical TEXT NOT NULL,
                canonical_key TEXT NOT NULL,
                language TEXT NOT NULL,
                origin TEXT NOT NULL CHECK (origin IN ('manual', 'automatic')),
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                UNIQUE(language, canonical_key)
            );
            CREATE TABLE dictionary_variants (
                id TEXT PRIMARY KEY NOT NULL,
                entry_id TEXT NOT NULL REFERENCES dictionary_entries(id) ON DELETE CASCADE,
                recognized_as TEXT NOT NULL,
                variant_key TEXT NOT NULL,
                created_at_ms INTEGER NOT NULL,
                UNIQUE(entry_id, variant_key)
            );
            CREATE TABLE term_observations (
                dictation_id TEXT NOT NULL,
                language TEXT NOT NULL,
                canonical TEXT NOT NULL,
                canonical_key TEXT NOT NULL,
                recognized_as TEXT NOT NULL,
                variant_key TEXT NOT NULL,
                occurrence_count INTEGER NOT NULL CHECK (occurrence_count > 0),
                observed_at_ms INTEGER NOT NULL,
                PRIMARY KEY(dictation_id, language, canonical_key, variant_key)
            );
            CREATE INDEX term_observations_candidate
                ON term_observations(language, canonical_key, variant_key, observed_at_ms);
            CREATE TABLE dictionary_candidates (
                language TEXT NOT NULL,
                canonical_key TEXT NOT NULL,
                variant_key TEXT NOT NULL,
                occurrence_count INTEGER NOT NULL,
                dictation_count INTEGER NOT NULL,
                last_observed_at_ms INTEGER NOT NULL,
                PRIMARY KEY(language, canonical_key, variant_key)
            );
            CREATE TABLE dictionary_suppressions (
                language TEXT NOT NULL,
                canonical_key TEXT NOT NULL,
                suppressed_until_ms INTEGER NOT NULL,
                PRIMARY KEY(language, canonical_key)
            );
            CREATE TABLE installed_models (
                id TEXT PRIMARY KEY NOT NULL,
                provider_type TEXT NOT NULL,
                model TEXT NOT NULL,
                version TEXT NOT NULL,
                path TEXT NOT NULL,
                installed_at_ms INTEGER NOT NULL,
                last_verified_at_ms INTEGER
            );
            PRAGMA user_version = 3;",
        )
        .map_err(unavailable)?;
    transaction.commit().map_err(unavailable)
}

fn migrate_settings(connection: &mut Connection) -> Result<(), StorageError> {
    let transaction = connection.transaction().map_err(unavailable)?;
    transaction
        .execute_batch(
            "CREATE TABLE app_settings (
                singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                history_enabled INTEGER NOT NULL CHECK (history_enabled IN (0, 1)),
                history_retention_days INTEGER CHECK (
                    history_retention_days IS NULL OR history_retention_days IN (1, 7, 30)
                ),
                automatic_dictionary_learning INTEGER NOT NULL
                    CHECK (automatic_dictionary_learning IN (0, 1))
            );
            INSERT INTO app_settings (
                singleton, history_enabled, history_retention_days,
                automatic_dictionary_learning
            ) VALUES (1, 1, 7, 1);
            PRAGMA user_version = 1;",
        )
        .map_err(unavailable)?;
    transaction.commit().map_err(unavailable)
}

fn migrate_history(connection: &mut Connection) -> Result<(), StorageError> {
    let transaction = connection.transaction().map_err(unavailable)?;
    transaction
        .execute_batch(
            "CREATE TABLE transcript_history (
                id TEXT PRIMARY KEY NOT NULL,
                created_at_ms INTEGER NOT NULL,
                crypto_version INTEGER NOT NULL,
                payload_version INTEGER NOT NULL,
                nonce BLOB NOT NULL,
                ciphertext BLOB NOT NULL
            );
            CREATE INDEX transcript_history_order
                ON transcript_history(created_at_ms DESC, id DESC);
            CREATE TABLE history_key_validation (
                singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                nonce BLOB NOT NULL,
                ciphertext BLOB NOT NULL
            );
            PRAGMA user_version = 2;",
        )
        .map_err(unavailable)?;
    transaction.commit().map_err(unavailable)
}
