use rusqlite::Connection;
use template_app::{StorageError, dictionary_comparison_key};

use super::unavailable;

const CURRENT_SCHEMA_VERSION: u32 = 4;

pub(super) fn apply(connection: &mut Connection) -> Result<(), StorageError> {
    let version: u32 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(unavailable)?;
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
    Ok(())
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
