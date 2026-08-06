use rusqlite::Connection;
use template_app::StorageError;

use super::{app_settings_has_column, unavailable};

const ADD_SAYMORE_THEME_MIGRATION: &str = "ALTER TABLE app_settings RENAME TO app_settings_v17;
     CREATE TABLE app_settings (
        singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
        history_enabled INTEGER NOT NULL CHECK (history_enabled IN (0, 1)),
        history_retention_days INTEGER CHECK (
            history_retention_days IS NULL OR history_retention_days IN (1, 7, 30)
        ),
        automatic_dictionary_learning INTEGER NOT NULL
            CHECK (automatic_dictionary_learning IN (0, 1)),
        preferred_microphone_id TEXT,
        preferred_microphone_name TEXT,
        diagnostics_logging_enabled INTEGER NOT NULL DEFAULT 0
            CHECK (diagnostics_logging_enabled IN (0, 1)),
        ui_language TEXT NOT NULL DEFAULT 'system'
            CHECK (ui_language IN ('system', 'en', 'zh-Hans')),
        automatic_update_checks INTEGER NOT NULL DEFAULT 0
            CHECK (automatic_update_checks IN (0, 1)),
        feedback_sounds_enabled INTEGER NOT NULL DEFAULT 1
            CHECK (feedback_sounds_enabled IN (0, 1)),
        copy_to_clipboard INTEGER NOT NULL DEFAULT 0
            CHECK (copy_to_clipboard IN (0, 1)),
        show_in_dock INTEGER NOT NULL DEFAULT 1
            CHECK (show_in_dock IN (0, 1)),
        dictation_paused INTEGER NOT NULL DEFAULT 0
            CHECK (dictation_paused IN (0, 1)),
        dictation_shortcut TEXT NOT NULL DEFAULT 'right-command',
        dictation_shortcuts TEXT NOT NULL DEFAULT 'right-command',
        onboarding_status TEXT NOT NULL DEFAULT 'not_started'
            CHECK (onboarding_status IN (
                'not_started', 'in_progress', 'completed', 'skipped'
            )),
        onboarding_step INTEGER NOT NULL DEFAULT 0
            CHECK (onboarding_step BETWEEN 0 AND 3),
        theme_id TEXT NOT NULL DEFAULT 'saymore'
            CHECK (theme_id IN (
                'saymore', 'warm-clay', 'lime-pulse', 'berry-graphite',
                'iris-mist', 'clear-sky'
            )),
        color_scheme TEXT NOT NULL DEFAULT 'system'
            CHECK (color_scheme IN ('system', 'light', 'dark')),
        mute_system_audio_enabled INTEGER NOT NULL DEFAULT 1
            CHECK (mute_system_audio_enabled IN (0, 1))
     );
     INSERT INTO app_settings (
        singleton, history_enabled, history_retention_days,
        automatic_dictionary_learning, preferred_microphone_id,
        preferred_microphone_name, diagnostics_logging_enabled,
        ui_language, automatic_update_checks, feedback_sounds_enabled,
        copy_to_clipboard, show_in_dock, dictation_paused,
        dictation_shortcut, dictation_shortcuts, onboarding_status,
        onboarding_step, theme_id, color_scheme, mute_system_audio_enabled
     )
     SELECT
        singleton, history_enabled, history_retention_days,
        automatic_dictionary_learning, preferred_microphone_id,
        preferred_microphone_name, diagnostics_logging_enabled,
        ui_language, automatic_update_checks, feedback_sounds_enabled,
        copy_to_clipboard, show_in_dock, dictation_paused,
        dictation_shortcut, dictation_shortcuts, onboarding_status,
        onboarding_step, theme_id, color_scheme, mute_system_audio_enabled
     FROM app_settings_v17;
     DROP TABLE app_settings_v17;";

const REPLACE_CLEAR_SKY_THEME_MIGRATION: &str =
    "ALTER TABLE app_settings RENAME TO app_settings_v18;
     CREATE TABLE app_settings (
        singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
        history_enabled INTEGER NOT NULL CHECK (history_enabled IN (0, 1)),
        history_retention_days INTEGER CHECK (
            history_retention_days IS NULL OR history_retention_days IN (1, 7, 30)
        ),
        automatic_dictionary_learning INTEGER NOT NULL
            CHECK (automatic_dictionary_learning IN (0, 1)),
        preferred_microphone_id TEXT,
        preferred_microphone_name TEXT,
        diagnostics_logging_enabled INTEGER NOT NULL DEFAULT 0
            CHECK (diagnostics_logging_enabled IN (0, 1)),
        ui_language TEXT NOT NULL DEFAULT 'system'
            CHECK (ui_language IN ('system', 'en', 'zh-Hans')),
        automatic_update_checks INTEGER NOT NULL DEFAULT 0
            CHECK (automatic_update_checks IN (0, 1)),
        feedback_sounds_enabled INTEGER NOT NULL DEFAULT 1
            CHECK (feedback_sounds_enabled IN (0, 1)),
        copy_to_clipboard INTEGER NOT NULL DEFAULT 0
            CHECK (copy_to_clipboard IN (0, 1)),
        show_in_dock INTEGER NOT NULL DEFAULT 1
            CHECK (show_in_dock IN (0, 1)),
        dictation_paused INTEGER NOT NULL DEFAULT 0
            CHECK (dictation_paused IN (0, 1)),
        dictation_shortcut TEXT NOT NULL DEFAULT 'right-command',
        dictation_shortcuts TEXT NOT NULL DEFAULT 'right-command',
        onboarding_status TEXT NOT NULL DEFAULT 'not_started'
            CHECK (onboarding_status IN (
                'not_started', 'in_progress', 'completed', 'skipped'
            )),
        onboarding_step INTEGER NOT NULL DEFAULT 0
            CHECK (onboarding_step BETWEEN 0 AND 3),
        theme_id TEXT NOT NULL DEFAULT 'saymore'
            CHECK (theme_id IN (
                'saymore', 'warm-clay', 'lime-pulse', 'berry-graphite',
                'iris-mist', 'sunlit-gold'
            )),
        color_scheme TEXT NOT NULL DEFAULT 'system'
            CHECK (color_scheme IN ('system', 'light', 'dark')),
        mute_system_audio_enabled INTEGER NOT NULL DEFAULT 1
            CHECK (mute_system_audio_enabled IN (0, 1))
     );
     INSERT INTO app_settings (
        singleton, history_enabled, history_retention_days,
        automatic_dictionary_learning, preferred_microphone_id,
        preferred_microphone_name, diagnostics_logging_enabled,
        ui_language, automatic_update_checks, feedback_sounds_enabled,
        copy_to_clipboard, show_in_dock, dictation_paused,
        dictation_shortcut, dictation_shortcuts, onboarding_status,
        onboarding_step, theme_id, color_scheme, mute_system_audio_enabled
     )
     SELECT
        singleton, history_enabled, history_retention_days,
        automatic_dictionary_learning, preferred_microphone_id,
        preferred_microphone_name, diagnostics_logging_enabled,
        ui_language, automatic_update_checks, feedback_sounds_enabled,
        copy_to_clipboard, show_in_dock, dictation_paused,
        dictation_shortcut, dictation_shortcuts, onboarding_status,
        onboarding_step,
        CASE theme_id WHEN 'clear-sky' THEN 'sunlit-gold' ELSE theme_id END,
        color_scheme, mute_system_audio_enabled
     FROM app_settings_v18;
     DROP TABLE app_settings_v18;";

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

pub(super) fn add_saymore_theme(
    connection: &mut Connection,
    fresh_install: bool,
) -> Result<(), StorageError> {
    let transaction = connection.transaction().map_err(unavailable)?;
    transaction
        .execute_batch(ADD_SAYMORE_THEME_MIGRATION)
        .map_err(unavailable)?;
    if fresh_install {
        transaction
            .execute(
                "UPDATE app_settings SET theme_id = 'saymore' WHERE singleton = 1",
                [],
            )
            .map_err(unavailable)?;
    }
    transaction
        .execute_batch("PRAGMA user_version = 18;")
        .map_err(unavailable)?;
    transaction.commit().map_err(unavailable)
}

pub(super) fn replace_clear_sky_theme(connection: &mut Connection) -> Result<(), StorageError> {
    let transaction = connection.transaction().map_err(unavailable)?;
    transaction
        .execute_batch(REPLACE_CLEAR_SKY_THEME_MIGRATION)
        .map_err(unavailable)?;
    transaction
        .execute_batch("PRAGMA user_version = 19;")
        .map_err(unavailable)?;
    transaction.commit().map_err(unavailable)
}

pub(super) fn add_usage_aggregates(connection: &mut Connection) -> Result<(), StorageError> {
    let transaction = connection.transaction().map_err(unavailable)?;
    transaction
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS usage_daily (
                local_date TEXT PRIMARY KEY,
                audio_duration_ms INTEGER NOT NULL CHECK (audio_duration_ms >= 0),
                character_count INTEGER NOT NULL CHECK (character_count >= 0)
             );
             CREATE TABLE IF NOT EXISTS usage_recorded_dictations (
                dictation_id TEXT PRIMARY KEY CHECK (length(dictation_id) > 0)
             );
             CREATE TABLE IF NOT EXISTS usage_aggregation_state (
                singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                history_backfilled INTEGER NOT NULL CHECK (history_backfilled IN (0, 1))
             );
             INSERT OR IGNORE INTO usage_aggregation_state(singleton, history_backfilled)
             VALUES (1, 0);
             PRAGMA user_version = 20;",
        )
        .map_err(unavailable)?;
    transaction.commit().map_err(unavailable)
}

pub(super) fn add_dictionary_assist_settings(
    connection: &mut Connection,
) -> Result<(), StorageError> {
    let transaction = connection.transaction().map_err(unavailable)?;
    if !app_settings_has_column(&transaction, "dictionary_assist_enabled")? {
        transaction
            .execute_batch(
                "ALTER TABLE app_settings
                 ADD COLUMN dictionary_assist_enabled INTEGER NOT NULL DEFAULT 0
                 CHECK (dictionary_assist_enabled IN (0, 1));
                 ALTER TABLE app_settings
                 ADD COLUMN dictionary_assist_last_success_at_ms INTEGER;",
            )
            .map_err(unavailable)?;
    }
    transaction
        .execute_batch("PRAGMA user_version = 21;")
        .map_err(unavailable)?;
    transaction.commit().map_err(unavailable)
}

pub(super) fn separate_dictionary_evidence_kinds(
    connection: &mut Connection,
) -> Result<(), StorageError> {
    let transaction = connection.transaction().map_err(unavailable)?;
    transaction
        .execute_batch(
            "ALTER TABLE term_observations RENAME TO term_observations_without_evidence_kind;
             ALTER TABLE dictionary_candidates RENAME TO dictionary_candidates_without_evidence_kind;
             DROP INDEX term_observations_candidate;
             CREATE TABLE term_observations (
                dictation_id TEXT NOT NULL,
                language TEXT NOT NULL,
                canonical TEXT NOT NULL,
                canonical_key TEXT NOT NULL,
                evidence_kind TEXT NOT NULL CHECK (evidence_kind IN (
                    'user_revision', 'vocabulary_suggestion'
                )),
                occurrence_count INTEGER NOT NULL CHECK (occurrence_count > 0),
                observed_at_ms INTEGER NOT NULL,
                PRIMARY KEY(dictation_id, language, canonical_key, evidence_kind)
             );
             CREATE INDEX term_observations_candidate
                ON term_observations(
                    language, canonical_key, evidence_kind, observed_at_ms
                );
             INSERT INTO term_observations(
                dictation_id, language, canonical, canonical_key,
                evidence_kind, occurrence_count, observed_at_ms
             )
             SELECT dictation_id, language, canonical, canonical_key,
                    'user_revision', occurrence_count, observed_at_ms
             FROM term_observations_without_evidence_kind;
             CREATE TABLE dictionary_candidates (
                language TEXT NOT NULL,
                canonical_key TEXT NOT NULL,
                evidence_kind TEXT NOT NULL CHECK (evidence_kind IN (
                    'user_revision', 'vocabulary_suggestion'
                )),
                occurrence_count INTEGER NOT NULL,
                dictation_count INTEGER NOT NULL,
                last_observed_at_ms INTEGER NOT NULL,
                PRIMARY KEY(language, canonical_key, evidence_kind)
             );
             INSERT INTO dictionary_candidates(
                language, canonical_key, evidence_kind, occurrence_count,
                dictation_count, last_observed_at_ms
             )
             SELECT language, canonical_key, 'user_revision', occurrence_count,
                    dictation_count, last_observed_at_ms
             FROM dictionary_candidates_without_evidence_kind;
             DROP TABLE term_observations_without_evidence_kind;
             DROP TABLE dictionary_candidates_without_evidence_kind;
             PRAGMA user_version = 22;",
        )
        .map_err(unavailable)?;
    transaction.commit().map_err(unavailable)
}

pub(super) fn make_dictionary_evidence_idempotent(
    connection: &mut Connection,
) -> Result<(), StorageError> {
    let transaction = connection.transaction().map_err(unavailable)?;
    transaction
        .execute_batch(
            "ALTER TABLE term_observations RENAME TO term_observations_repeated;
             ALTER TABLE dictionary_candidates RENAME TO dictionary_candidates_by_kind;
             DROP INDEX term_observations_candidate;
             CREATE TABLE term_observations (
                dictation_id TEXT NOT NULL,
                language TEXT NOT NULL,
                canonical TEXT NOT NULL,
                canonical_key TEXT NOT NULL,
                evidence_kind TEXT NOT NULL CHECK(evidence_kind IN (
                    'user_revision', 'vocabulary_suggestion'
                )),
                decision TEXT NOT NULL CHECK(decision IN ('accept', 'uncertain')),
                confidence INTEGER NOT NULL CHECK(confidence BETWEEN 60 AND 100),
                occurrence_count INTEGER NOT NULL DEFAULT 1 CHECK(occurrence_count = 1),
                observed_at_ms INTEGER NOT NULL,
                PRIMARY KEY(dictation_id, language, canonical_key)
             );
             CREATE INDEX term_observations_candidate
                ON term_observations(language, canonical_key, observed_at_ms);
             INSERT INTO term_observations(
                dictation_id, language, canonical, canonical_key, evidence_kind,
                decision, confidence, occurrence_count, observed_at_ms
             )
             SELECT dictation_id, language, MAX(canonical), canonical_key,
                    CASE WHEN SUM(evidence_kind = 'user_revision') > 0
                         THEN 'user_revision' ELSE 'vocabulary_suggestion' END,
                    'uncertain', 60, 1, MAX(observed_at_ms)
             FROM term_observations_repeated
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
             SELECT language, canonical_key, COUNT(*),
                    COUNT(DISTINCT dictation_id), MAX(observed_at_ms)
             FROM term_observations
             GROUP BY language, canonical_key;
             DROP TABLE term_observations_repeated;
             DROP TABLE dictionary_candidates_by_kind;
             PRAGMA user_version = 23;",
        )
        .map_err(unavailable)?;
    transaction.commit().map_err(unavailable)
}

pub(super) fn add_dictionary_assist_consent(
    connection: &mut Connection,
) -> Result<(), StorageError> {
    let transaction = connection.transaction().map_err(unavailable)?;
    if !app_settings_has_column(&transaction, "dictionary_assist_consent_fingerprint")? {
        transaction
            .execute_batch(
                "ALTER TABLE app_settings
                 ADD COLUMN dictionary_assist_consent_fingerprint TEXT;",
            )
            .map_err(unavailable)?;
    }
    transaction
        .execute_batch("PRAGMA user_version = 24;")
        .map_err(unavailable)?;
    transaction.commit().map_err(unavailable)
}

pub(super) fn add_model_download_queue(connection: &mut Connection) -> Result<(), StorageError> {
    let transaction = connection.transaction().map_err(unavailable)?;
    transaction
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS model_download_queue (
                sequence INTEGER PRIMARY KEY AUTOINCREMENT,
                model_id TEXT NOT NULL UNIQUE CHECK(length(trim(model_id)) > 0)
             );
             PRAGMA user_version = 25;",
        )
        .map_err(unavailable)?;
    transaction.commit().map_err(unavailable)
}
