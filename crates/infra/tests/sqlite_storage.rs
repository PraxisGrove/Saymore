#![allow(clippy::panic_in_result_fn)]

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use template_app::{
    ColorSchemePreference, DiagnosticEventStore, DictionaryOrigin, DictionaryStore,
    HistoryDelivery, HistoryRecord, HistoryRefinement, HistoryRetention, HistoryStore,
    InstalledModel, InstalledModelStore, LocalSettings, LocalSettingsStore, NewDictionaryEntry,
    NewHistoryRecord, OnboardingStatus, OnboardingStep, SecretStore, SecretStoreError,
    StorageError, ThemeId, UiLanguagePreference, UsageStore, VocabularySuggestionSettingsStore,
};
use template_infra::SqliteStorage;
#[cfg(target_os = "windows")]
use template_infra::WindowsShortcut;

#[derive(Default)]
struct MemorySecretStore {
    key: Mutex<Option<Vec<u8>>>,
    fail_save: AtomicBool,
    access_count: AtomicUsize,
}

impl SecretStore for MemorySecretStore {
    fn load_history_key(&self) -> Result<Option<Vec<u8>>, SecretStoreError> {
        self.access_count.fetch_add(1, Ordering::Relaxed);
        self.key
            .lock()
            .map(|key| key.clone())
            .map_err(|_| SecretStoreError::Unavailable("test key lock poisoned".to_owned()))
    }

    fn save_history_key(&self, key: &[u8]) -> Result<(), SecretStoreError> {
        self.access_count.fetch_add(1, Ordering::Relaxed);
        if self.fail_save.load(Ordering::Relaxed) {
            return Err(SecretStoreError::Unavailable(
                "injected secret save failure".to_owned(),
            ));
        }
        self.key
            .lock()
            .map(|mut stored| *stored = Some(key.to_vec()))
            .map_err(|_| SecretStoreError::Unavailable("test key lock poisoned".to_owned()))
    }

    fn delete_history_key(&self) -> Result<(), SecretStoreError> {
        self.access_count.fetch_add(1, Ordering::Relaxed);
        self.key
            .lock()
            .map(|mut stored| *stored = None)
            .map_err(|_| SecretStoreError::Unavailable("test key lock poisoned".to_owned()))
    }
}

#[test]
fn secret_store_is_not_accessed_until_history_needs_encryption()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let secrets = Arc::new(MemorySecretStore::default());
    let store = SqliteStorage::start(directory.path().join("saymore.sqlite3"), secrets.clone())?;

    store.load_settings()?;
    store.list_dictionary()?;
    store.cleanup_history(1_000)?;
    assert_eq!(0, secrets.access_count.load(Ordering::Relaxed));

    store.insert_history(history_record("first", 1_000, "首次历史"))?;
    assert!(secrets.access_count.load(Ordering::Relaxed) > 0);
    Ok(())
}

#[test]
fn diagnostic_events_round_trip_in_chronological_order() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let store = SqliteStorage::start(
        directory.path().join("saymore.sqlite3"),
        Arc::new(MemorySecretStore::default()),
    )?;

    store.record_diagnostic_event("application.started")?;
    store.record_diagnostic_event("microphone.devices_listed")?;
    assert!(
        store
            .record_diagnostic_event("transcript=private value")
            .is_err()
    );

    assert_eq!(
        vec![
            "application.started".to_owned(),
            "microphone.devices_listed".to_owned()
        ],
        store.diagnostic_events(10)?
    );
    assert_eq!(
        vec!["microphone.devices_listed".to_owned()],
        store.diagnostic_events(1)?
    );
    Ok(())
}

fn history_record(id: &str, created_at_ms: i64, final_text: &str) -> NewHistoryRecord {
    NewHistoryRecord {
        id: id.to_owned(),
        created_at_ms,
        final_text: final_text.to_owned(),
        raw_asr_text: None,
        llm_refined_text: None,
        audio_duration_ms: 1_500,
        language: Some("zh-Hans".to_owned()),
        delivery: HistoryDelivery::Delivered,
        refinement: HistoryRefinement::Completed,
        asr_provider_id: Some("asr-primary".to_owned()),
        llm_provider_id: Some("llm-primary".to_owned()),
        asr_model: Some("asr-model".to_owned()),
        llm_model: Some("llm-model".to_owned()),
    }
}

fn expected_fresh_settings() -> LocalSettings {
    #[cfg(target_os = "windows")]
    {
        LocalSettings {
            dictation_shortcuts: vec!["windows:right-alt".to_owned()],
            ..LocalSettings::default()
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        LocalSettings::default()
    }
}

#[test]
fn settings_are_typed_and_persisted_across_restarts() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("saymore.sqlite3");
    let secrets = Arc::new(MemorySecretStore::default());
    let store = SqliteStorage::start(path.clone(), secrets.clone())?;

    assert_eq!(expected_fresh_settings(), store.load_settings()?);
    let mut changed = LocalSettings {
        history_enabled: false,
        history_retention: HistoryRetention::ThirtyDays,
        dictionary_assist_enabled: true,
        dictionary_assist_consent_fingerprint: Some("provider-consent-v1".to_owned()),
        dictionary_assist_last_success_at_ms: Some(1_722_000_000_000),
        preferred_microphone_id: Some("coreaudio:BuiltInMicrophoneDevice".to_owned()),
        preferred_microphone_name: Some("MacBook 麦克风".to_owned()),
        diagnostics_logging_enabled: true,
        ui_language: UiLanguagePreference::English,
        theme: ThemeId::SunlitGold,
        color_scheme: ColorSchemePreference::Dark,
        automatic_update_checks: true,
        feedback_sounds_enabled: false,
        mute_system_audio_enabled: false,
        copy_to_clipboard: true,
        show_in_dock: false,
        dictation_paused: true,
        dictation_shortcuts: Vec::new(),
        onboarding_status: OnboardingStatus::InProgress,
        onboarding_step: OnboardingStep::Accessibility,
    };
    store.save_settings(changed.clone())?;
    drop(store);

    let reopened = SqliteStorage::start(path.clone(), secrets.clone())?;
    assert_eq!(changed, reopened.load_settings()?);
    changed.theme = ThemeId::Saymore;
    reopened.save_settings(changed.clone())?;
    drop(reopened);

    let reopened_with_default_theme = SqliteStorage::start(path, secrets)?;
    assert_eq!(changed, reopened_with_default_theme.load_settings()?);
    Ok(())
}

#[test]
fn vocabulary_checkpoint_advances_only_for_the_active_consent()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let store = SqliteStorage::start(
        directory.path().join("saymore.sqlite3"),
        Arc::new(MemorySecretStore::default()),
    )?;
    let mut settings = store.load_settings()?;
    settings.dictionary_assist_enabled = true;
    settings.dictionary_assist_consent_fingerprint = Some("active-consent".to_owned());
    store.save_settings(settings)?;

    assert!(!store.record_vocabulary_suggestion_success("stale-consent", 40)?);
    assert_eq!(
        None,
        store.load_settings()?.dictionary_assist_last_success_at_ms
    );
    assert!(store.record_vocabulary_suggestion_success("active-consent", 42)?);
    assert_eq!(
        Some(42),
        store.load_settings()?.dictionary_assist_last_success_at_ms
    );
    Ok(())
}

#[test]
fn v22_duplicate_dictionary_channels_migrate_to_one_dictation_term_evidence()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("saymore.sqlite3");
    let secrets = Arc::new(MemorySecretStore::default());
    drop(SqliteStorage::start(path.clone(), secrets.clone())?);
    let connection = rusqlite::Connection::open(&path)?;
    connection.execute_batch(
        "DROP INDEX term_observations_candidate;
         DROP TABLE term_observations;
         DROP TABLE dictionary_candidates;
         CREATE TABLE term_observations (
            dictation_id TEXT NOT NULL,
            language TEXT NOT NULL,
            canonical TEXT NOT NULL,
            canonical_key TEXT NOT NULL,
            evidence_kind TEXT NOT NULL,
            occurrence_count INTEGER NOT NULL,
            observed_at_ms INTEGER NOT NULL,
            PRIMARY KEY(dictation_id, language, canonical_key, evidence_kind)
         );
         CREATE INDEX term_observations_candidate
            ON term_observations(language, canonical_key, evidence_kind, observed_at_ms);
         CREATE TABLE dictionary_candidates (
            language TEXT NOT NULL,
            canonical_key TEXT NOT NULL,
            evidence_kind TEXT NOT NULL,
            occurrence_count INTEGER NOT NULL,
            dictation_count INTEGER NOT NULL,
            last_observed_at_ms INTEGER NOT NULL,
            PRIMARY KEY(language, canonical_key, evidence_kind)
         );
         INSERT INTO term_observations VALUES
            ('dictation', 'zh-CN', '千问', '千问', 'user_revision', 2, 10),
            ('dictation', 'zh-CN', '千问', '千问', 'vocabulary_suggestion', 3, 20);
         INSERT INTO dictionary_candidates VALUES
            ('zh-CN', '千问', 'user_revision', 2, 1, 10),
            ('zh-CN', '千问', 'vocabulary_suggestion', 3, 1, 20);
         PRAGMA user_version = 22;",
    )?;
    drop(connection);

    drop(SqliteStorage::start(path.clone(), secrets)?);
    let connection = rusqlite::Connection::open(path)?;
    let observation: (u32, u32, String) = connection.query_row(
        "SELECT COUNT(*), SUM(occurrence_count), evidence_kind
         FROM term_observations WHERE canonical_key = '千问'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    assert_eq!((1, 1, "user_revision".to_owned()), observation);
    let candidate: (u32, u32) = connection.query_row(
        "SELECT occurrence_count, dictation_count
         FROM dictionary_candidates WHERE canonical_key = '千问'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    assert_eq!((1, 1), candidate);
    Ok(())
}

#[test]
fn clear_sky_settings_migrate_to_sunlit_gold() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("saymore.sqlite3");
    let secrets = Arc::new(MemorySecretStore::default());
    drop(SqliteStorage::start(path.clone(), secrets.clone())?);

    let connection = rusqlite::Connection::open(&path)?;
    connection.execute_batch(
        "PRAGMA writable_schema = ON;
         UPDATE sqlite_schema
         SET sql = replace(sql, '''sunlit-gold''', '''clear-sky''')
         WHERE type = 'table' AND name = 'app_settings';
         PRAGMA writable_schema = OFF;
         PRAGMA user_version = 18;",
    )?;
    drop(connection);

    let connection = rusqlite::Connection::open(&path)?;
    connection.execute(
        "UPDATE app_settings SET theme_id = 'clear-sky' WHERE singleton = 1",
        [],
    )?;
    drop(connection);

    let store = SqliteStorage::start(path.clone(), secrets)?;
    assert_eq!(ThemeId::SunlitGold, store.load_settings()?.theme);
    drop(store);

    let connection = rusqlite::Connection::open(path)?;
    let stored_theme: String = connection.query_row(
        "SELECT theme_id FROM app_settings WHERE singleton = 1",
        [],
        |row| row.get(0),
    )?;
    let version: u32 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    assert_eq!("sunlit-gold", stored_theme);
    assert_eq!(24, version);
    Ok(())
}

#[cfg(target_os = "windows")]
#[test]
fn fresh_windows_install_has_a_registerable_platform_default()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let store = SqliteStorage::start(
        directory.path().join("saymore.sqlite3"),
        Arc::new(MemorySecretStore::default()),
    )?;
    let settings = store.load_settings()?;
    assert_eq!(1, settings.dictation_shortcuts.len());
    assert!(
        settings
            .dictation_shortcuts
            .first()
            .is_some_and(|value| WindowsShortcut::from_storage_value(value).is_ok())
    );
    Ok(())
}

#[test]
fn existing_installations_do_not_receive_first_run_onboarding()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("saymore.sqlite3");
    let connection = rusqlite::Connection::open(&path)?;
    connection.execute_batch(
        "CREATE TABLE app_settings (
            singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
            history_enabled INTEGER NOT NULL,
            history_retention_days INTEGER,
            automatic_dictionary_learning INTEGER NOT NULL,
            preferred_microphone_id TEXT,
            preferred_microphone_name TEXT,
            diagnostics_logging_enabled INTEGER NOT NULL DEFAULT 0,
            ui_language TEXT NOT NULL DEFAULT 'system',
            automatic_update_checks INTEGER NOT NULL DEFAULT 0,
            feedback_sounds_enabled INTEGER NOT NULL DEFAULT 1,
            copy_to_clipboard INTEGER NOT NULL DEFAULT 0,
            show_in_dock INTEGER NOT NULL DEFAULT 1,
            dictation_paused INTEGER NOT NULL DEFAULT 0,
            dictation_shortcut TEXT NOT NULL DEFAULT 'right-command',
            dictation_shortcuts TEXT NOT NULL DEFAULT 'right-command'
        );
        INSERT INTO app_settings (
            singleton, history_enabled, history_retention_days,
            automatic_dictionary_learning
        ) VALUES (1, 1, 7, 1);
        CREATE TABLE term_observations (
            dictation_id TEXT NOT NULL,
            language TEXT NOT NULL,
            canonical TEXT NOT NULL,
            canonical_key TEXT NOT NULL,
            occurrence_count INTEGER NOT NULL,
            observed_at_ms INTEGER NOT NULL,
            PRIMARY KEY(dictation_id, language, canonical_key)
        );
        CREATE INDEX term_observations_candidate
            ON term_observations(language, canonical_key, observed_at_ms);
        CREATE TABLE dictionary_candidates (
            language TEXT NOT NULL,
            canonical_key TEXT NOT NULL,
            occurrence_count INTEGER NOT NULL,
            dictation_count INTEGER NOT NULL,
            last_observed_at_ms INTEGER NOT NULL,
            PRIMARY KEY(language, canonical_key)
        );
        PRAGMA user_version = 12;",
    )?;
    drop(connection);

    let store = SqliteStorage::start(path, Arc::new(MemorySecretStore::default()))?;

    assert_eq!(
        OnboardingStatus::Completed,
        store.load_settings()?.onboarding_status
    );
    assert_eq!(ThemeId::LimePulse, store.load_settings()?.theme);
    assert_eq!(
        ColorSchemePreference::System,
        store.load_settings()?.color_scheme
    );
    assert!(store.load_settings()?.mute_system_audio_enabled);
    Ok(())
}

#[test]
fn v7_settings_migrate_to_follow_system_language() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("saymore.sqlite3");
    let secrets = Arc::new(MemorySecretStore::default());
    drop(SqliteStorage::start(path.clone(), secrets.clone())?);

    let connection = rusqlite::Connection::open(&path)?;
    connection.execute_batch(
        "ALTER TABLE app_settings RENAME TO app_settings_v8;
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
                CHECK (diagnostics_logging_enabled IN (0, 1))
         );
         INSERT INTO app_settings
         SELECT singleton, history_enabled, history_retention_days,
                automatic_dictionary_learning, preferred_microphone_id,
                preferred_microphone_name, diagnostics_logging_enabled
         FROM app_settings_v8;
         DROP TABLE app_settings_v8;
         PRAGMA user_version = 7;",
    )?;
    drop(connection);

    let store = SqliteStorage::start(path, secrets)?;
    assert_eq!(
        UiLanguagePreference::System,
        store.load_settings()?.ui_language
    );
    Ok(())
}

#[test]
fn history_is_encrypted_and_uses_stable_keyset_pagination() -> Result<(), Box<dyn std::error::Error>>
{
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("saymore.sqlite3");
    let store = SqliteStorage::start(path.clone(), Arc::new(MemorySecretStore::default()))?;
    store.insert_history(history_record("older", 1_000, "第一条私密历史"))?;
    let mut newer = history_record("newer", 2_000, "第二条私密历史");
    newer.raw_asr_text = Some("第二条 ASR 原始结果".to_owned());
    newer.llm_refined_text = Some("第二条 LLM 润色结果".to_owned());
    store.insert_history(newer)?;

    let first = store.history_page(None, 1)?;
    assert_eq!(
        vec!["newer"],
        first
            .records
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        HistoryRecord {
            id: "newer".to_owned(),
            created_at_ms: 2_000,
            final_text: "第二条私密历史".to_owned(),
            raw_asr_text: Some("第二条 ASR 原始结果".to_owned()),
            llm_refined_text: Some("第二条 LLM 润色结果".to_owned()),
            audio_duration_ms: 1_500,
            language: Some("zh-Hans".to_owned()),
            delivery: HistoryDelivery::Delivered,
            refinement: HistoryRefinement::Completed,
            asr_provider_id: Some("asr-primary".to_owned()),
            llm_provider_id: Some("llm-primary".to_owned()),
            asr_model: Some("asr-model".to_owned()),
            llm_model: Some("llm-model".to_owned()),
        },
        first.records[0]
    );
    store.update_history_final_text("newer", "用户纠正后的最终文本")?;
    assert_eq!(
        HistoryRecord {
            final_text: "用户纠正后的最终文本".to_owned(),
            ..first.records[0].clone()
        },
        store.history_page(None, 1)?.records[0]
    );
    let second = store.history_page(first.next_cursor, 1)?;
    assert_eq!(
        vec!["older"],
        second
            .records
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>()
    );
    store.update_history_delivery("newer", HistoryDelivery::NotDelivered)?;
    assert_eq!(
        HistoryDelivery::NotDelivered,
        store.history_page(None, 1)?.records[0].delivery
    );

    drop(store);
    let database = std::fs::read(path)?;
    assert!(
        !database
            .windows("第一条私密历史".len())
            .any(|bytes| bytes == "第一条私密历史".as_bytes())
    );
    assert!(
        !database
            .windows("第二条私密历史".len())
            .any(|bytes| bytes == "第二条私密历史".as_bytes())
    );
    assert!(
        !database
            .windows("第二条 ASR 原始结果".len())
            .any(|bytes| bytes == "第二条 ASR 原始结果".as_bytes())
    );
    assert!(
        !database
            .windows("第二条 LLM 润色结果".len())
            .any(|bytes| bytes == "第二条 LLM 润色结果".as_bytes())
    );
    assert!(
        !database
            .windows("用户纠正后的最终文本".len())
            .any(|bytes| bytes == "用户纠正后的最终文本".as_bytes())
    );
    Ok(())
}

#[test]
fn usage_survives_history_clear_and_duplicate_inserts_count_once()
-> Result<(), Box<dyn std::error::Error>> {
    use chrono::{Local, TimeZone};

    let directory = tempfile::tempdir()?;
    let store = SqliteStorage::start(
        directory.path().join("saymore.sqlite3"),
        Arc::new(MemorySecretStore::default()),
    )?;
    let completed_at = Local
        .with_ymd_and_hms(2026, 8, 3, 10, 0, 0)
        .single()
        .ok_or("local test date is unavailable")?;
    let record = history_record("usage-id", completed_at.timestamp_millis(), "你好 Saymore");
    store.insert_history(record.clone())?;
    store.insert_history(record)?;

    let before_clear =
        store.usage_snapshot(completed_at.date_naive(), completed_at.date_naive())?;
    assert_eq!(1_500, before_clear.total_duration_ms);
    assert_eq!(9, before_clear.total_characters);
    assert_eq!(1, before_clear.days.len());

    store.clear_history()?;

    assert!(store.history_page(None, 50)?.records.is_empty());
    assert_eq!(
        before_clear,
        store.usage_snapshot(completed_at.date_naive(), completed_at.date_naive())?
    );
    Ok(())
}

#[test]
fn v19_history_backfills_usage_once() -> Result<(), Box<dyn std::error::Error>> {
    use chrono::{Local, TimeZone};

    let directory = tempfile::tempdir()?;
    let path = directory.path().join("saymore.sqlite3");
    let secrets = Arc::new(MemorySecretStore::default());
    let completed_at = Local
        .with_ymd_and_hms(2026, 8, 2, 10, 0, 0)
        .single()
        .ok_or("local test date is unavailable")?;
    let store = SqliteStorage::start(path.clone(), secrets.clone())?;
    store.insert_history(history_record(
        "legacy-usage",
        completed_at.timestamp_millis(),
        "旧数据",
    ))?;
    drop(store);

    let connection = rusqlite::Connection::open(&path)?;
    connection.execute_batch(
        "DROP TABLE usage_daily;
         DROP TABLE usage_recorded_dictations;
         DROP TABLE usage_aggregation_state;
         PRAGMA user_version = 19;",
    )?;
    drop(connection);

    let reopened = SqliteStorage::start(path, secrets)?;
    let first = reopened.usage_snapshot(completed_at.date_naive(), completed_at.date_naive())?;
    let second = reopened.usage_snapshot(completed_at.date_naive(), completed_at.date_naive())?;
    assert_eq!(1_500, first.total_duration_ms);
    assert_eq!(3, first.total_characters);
    assert_eq!(first, second);
    Ok(())
}

#[test]
fn history_search_filters_decrypted_text_and_keeps_stable_pages()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let store = SqliteStorage::start(
        directory.path().join("saymore.sqlite3"),
        Arc::new(MemorySecretStore::default()),
    )?;
    for index in 0..55 {
        let text = if index % 10 == 0 {
            format!("首页方案 {index}")
        } else {
            format!("其他记录 {index}")
        };
        store.insert_history(history_record(&format!("record-{index:02}"), index, &text))?;
    }

    let first = store.search_history_page(None, 3, " 首页方案 ")?;
    assert_eq!(
        vec!["record-50", "record-40", "record-30"],
        first
            .records
            .iter()
            .map(|record| record.id.as_str())
            .collect::<Vec<_>>()
    );
    let second = store.search_history_page(first.next_cursor, 3, "首页方案")?;
    assert_eq!(
        vec!["record-20", "record-10", "record-00"],
        second
            .records
            .iter()
            .map(|record| record.id.as_str())
            .collect::<Vec<_>>()
    );
    assert!(second.next_cursor.is_none());
    Ok(())
}

#[test]
fn rejects_history_rows_with_an_unknown_crypto_version() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("saymore.sqlite3");
    let secrets = Arc::new(MemorySecretStore::default());
    let store = SqliteStorage::start(path.clone(), secrets.clone())?;
    store.insert_history(history_record("future", 1_000, "未来格式"))?;
    drop(store);
    rusqlite::Connection::open(&path)?.execute(
        "UPDATE transcript_history SET crypto_version = 2 WHERE id = 'future'",
        [],
    )?;

    let reopened = SqliteStorage::start(path, secrets)?;
    assert!(matches!(
        reopened.history_page(None, 50),
        Err(StorageError::Invalid(_))
    ));
    Ok(())
}

#[test]
fn malformed_key_validation_data_is_reported_as_invalid_not_locked()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("saymore.sqlite3");
    let secrets = Arc::new(MemorySecretStore::default());
    let store = SqliteStorage::start(path.clone(), secrets.clone())?;
    store.insert_history(history_record("history-id", 1_000, "仍有密钥"))?;
    drop(store);
    rusqlite::Connection::open(&path)?.execute(
        "UPDATE history_key_validation SET nonce = X'00' WHERE singleton = 1",
        [],
    )?;

    let reopened = SqliteStorage::start(path, secrets)?;
    assert!(matches!(
        reopened.history_page(None, 50),
        Err(StorageError::Invalid(_))
    ));
    Ok(())
}

#[test]
fn corrupted_key_validation_ciphertext_is_distinguished_from_a_wrong_key()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("saymore.sqlite3");
    let secrets = Arc::new(MemorySecretStore::default());
    let store = SqliteStorage::start(path.clone(), secrets.clone())?;
    store.insert_history(history_record("history-id", 1_000, "密文仍然完好"))?;
    drop(store);
    let connection = rusqlite::Connection::open(&path)?;
    let mut ciphertext: Vec<u8> = connection.query_row(
        "SELECT ciphertext FROM history_key_validation WHERE singleton = 1",
        [],
        |row| row.get(0),
    )?;
    let first = ciphertext
        .first_mut()
        .ok_or("validation ciphertext empty")?;
    *first ^= 1;
    connection.execute(
        "UPDATE history_key_validation SET ciphertext = ?1 WHERE singleton = 1",
        [ciphertext],
    )?;
    drop(connection);

    let reopened = SqliteStorage::start(path, secrets)?;
    assert!(matches!(
        reopened.history_page(None, 50),
        Err(StorageError::Invalid(_))
    ));
    Ok(())
}

#[test]
fn existing_history_is_locked_when_its_key_is_missing() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("saymore.sqlite3");
    let secrets = Arc::new(MemorySecretStore::default());
    let store = SqliteStorage::start(path.clone(), secrets.clone())?;
    store.insert_history(history_record("history-id", 1_000, "无法恢复的历史"))?;
    drop(store);
    secrets.delete_history_key()?;

    let reopened = SqliteStorage::start(path, secrets)?;
    assert_eq!(
        Err(StorageError::HistoryLocked),
        reopened.history_page(None, 50)
    );
    assert_eq!(expected_fresh_settings(), reopened.load_settings()?);
    Ok(())
}

#[test]
fn explicit_history_reset_rotates_the_key_without_removing_dictionary_entries()
-> Result<(), Box<dyn std::error::Error>> {
    use chrono::{Local, TimeZone};

    let directory = tempfile::tempdir()?;
    let secrets = Arc::new(MemorySecretStore::default());
    let store = SqliteStorage::start(directory.path().join("saymore.sqlite3"), secrets.clone())?;
    store.insert_history(history_record("history-id", 1_000, "重置前历史"))?;
    let old_key = secrets.load_history_key()?.ok_or("history key missing")?;
    let dictionary_entry = store.upsert_dictionary(
        NewDictionaryEntry {
            canonical: "Saymore".to_owned(),
            language: "en".to_owned(),
            origin: DictionaryOrigin::Manual,
        },
        1_000,
    )?;
    let usage_date = Local
        .timestamp_millis_opt(1_000)
        .single()
        .ok_or("usage date is unavailable")?
        .date_naive();
    let usage_before_reset = store.usage_snapshot(usage_date, usage_date)?;

    store.reset_history()?;

    let new_key = secrets.load_history_key()?.ok_or("rotated key missing")?;
    assert_ne!(old_key, new_key);
    assert!(store.history_page(None, 50)?.records.is_empty());
    assert_eq!(vec![dictionary_entry], store.list_dictionary()?);
    assert_eq!(
        usage_before_reset,
        store.usage_snapshot(usage_date, usage_date)?
    );
    Ok(())
}

#[test]
fn failed_key_rotation_does_not_leave_old_history_with_an_overwritten_key()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let secrets = Arc::new(MemorySecretStore::default());
    let store = SqliteStorage::start(directory.path().join("saymore.sqlite3"), secrets.clone())?;
    store.insert_history(history_record("history-id", 1_000, "即将重置"))?;
    let dictionary_entry = store.upsert_dictionary(
        NewDictionaryEntry {
            canonical: "Saymore".to_owned(),
            language: "en".to_owned(),
            origin: DictionaryOrigin::Manual,
        },
        1_000,
    )?;
    secrets.fail_save.store(true, Ordering::Relaxed);

    assert!(matches!(
        store.reset_history(),
        Err(StorageError::Unavailable(_))
    ));
    assert!(matches!(
        store.history_page(None, 50),
        Err(StorageError::Unavailable(_))
    ));
    assert_eq!(vec![dictionary_entry], store.list_dictionary()?);
    assert!(matches!(
        store.insert_history(history_record("must-not-write", 2_000, "不能写入")),
        Err(StorageError::Unavailable(_))
    ));
    secrets.fail_save.store(false, Ordering::Relaxed);
    drop(store);
    let reopened = SqliteStorage::start(directory.path().join("saymore.sqlite3"), secrets)?;
    assert!(reopened.history_page(None, 50)?.records.is_empty());
    Ok(())
}

#[test]
fn shortening_retention_removes_expired_history_immediately()
-> Result<(), Box<dyn std::error::Error>> {
    use chrono::{Duration, Local, TimeZone};

    const DAY_MS: i64 = 86_400_000;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis() as i64;
    let directory = tempfile::tempdir()?;
    let store = SqliteStorage::start(
        directory.path().join("saymore.sqlite3"),
        Arc::new(MemorySecretStore::default()),
    )?;
    store.insert_history(history_record("expired", now_ms - DAY_MS * 2, "过期"))?;
    store.insert_history(history_record("current", now_ms, "保留"))?;
    store.save_settings(LocalSettings {
        history_retention: HistoryRetention::OneDay,
        ..LocalSettings::default()
    })?;

    assert_eq!(0, store.cleanup_history(now_ms)?);
    let page = store.history_page(None, 50)?;
    assert_eq!(
        vec!["current"],
        page.records
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>()
    );
    let today = Local
        .timestamp_millis_opt(now_ms)
        .single()
        .ok_or("usage date is unavailable")?
        .date_naive();
    let usage = store.usage_snapshot(today - Duration::days(3), today)?;
    assert_eq!(3_000, usage.total_duration_ms);
    assert_eq!(4, usage.total_characters);
    Ok(())
}

#[test]
fn manual_dictionary_entries_merge_normalized_duplicates() -> Result<(), Box<dyn std::error::Error>>
{
    let directory = tempfile::tempdir()?;
    let store = SqliteStorage::start(
        directory.path().join("saymore.sqlite3"),
        Arc::new(MemorySecretStore::default()),
    )?;
    store.upsert_dictionary(
        NewDictionaryEntry {
            canonical: "openai".to_owned(),
            language: "en".to_owned(),
            origin: DictionaryOrigin::Automatic,
        },
        1_000,
    )?;
    let updated = store.upsert_dictionary(
        NewDictionaryEntry {
            canonical: "OpenAI".to_owned(),
            language: "en".to_owned(),
            origin: DictionaryOrigin::Manual,
        },
        2_000,
    )?;

    assert_eq!("OpenAI", updated.canonical);
    assert_eq!(DictionaryOrigin::Manual, updated.origin);
    assert_eq!(vec![updated], store.list_dictionary()?);
    Ok(())
}

#[test]
fn dictionary_spelling_updates_preserve_entry_identity_and_provenance()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let store = SqliteStorage::start(
        directory.path().join("saymore.sqlite3"),
        Arc::new(MemorySecretStore::default()),
    )?;
    let original = store.upsert_dictionary(
        NewDictionaryEntry {
            canonical: "paraformer".to_owned(),
            language: "en".to_owned(),
            origin: DictionaryOrigin::Automatic,
        },
        1_000,
    )?;

    let updated = store.update_dictionary(&original.id, "  ParaFormer  ", 2_000)?;

    assert_eq!(original.id, updated.id);
    assert_eq!("ParaFormer", updated.canonical);
    assert_eq!(original.language, updated.language);
    assert_eq!(original.origin, updated.origin);
    assert_eq!(original.created_at_ms, updated.created_at_ms);
    assert_eq!(2_000, updated.updated_at_ms);
    assert_eq!(vec![updated], store.list_dictionary()?);
    Ok(())
}

#[test]
fn dictionary_spelling_updates_reject_conflicts_and_unknown_entries()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let store = SqliteStorage::start(
        directory.path().join("saymore.sqlite3"),
        Arc::new(MemorySecretStore::default()),
    )?;
    let first = store.upsert_dictionary(
        NewDictionaryEntry {
            canonical: "Paraformer".to_owned(),
            language: "en".to_owned(),
            origin: DictionaryOrigin::Manual,
        },
        1_000,
    )?;
    let second = store.upsert_dictionary(
        NewDictionaryEntry {
            canonical: "OpenAI".to_owned(),
            language: "en".to_owned(),
            origin: DictionaryOrigin::Manual,
        },
        1_000,
    )?;

    assert!(matches!(
        store.update_dictionary(&first.id, "openai", 2_000),
        Err(StorageError::Invalid(_))
    ));
    assert!(matches!(
        store.update_dictionary("missing", "Other", 2_000),
        Err(StorageError::Invalid(_))
    ));
    assert_eq!(vec![second, first], store.list_dictionary()?);
    Ok(())
}

#[test]
fn dictionary_identity_preserves_token_boundaries_across_v3_migration()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("saymore.sqlite3");
    let secrets = Arc::new(MemorySecretStore::default());
    let store = SqliteStorage::start(path.clone(), secrets.clone())?;
    store.upsert_dictionary(
        NewDictionaryEntry {
            canonical: "Open AI".to_owned(),
            language: "en".to_owned(),
            origin: DictionaryOrigin::Manual,
        },
        1_000,
    )?;
    drop(store);

    let connection = rusqlite::Connection::open(&path)?;
    connection.execute(
        "UPDATE dictionary_entries SET canonical_key = 'openai' WHERE canonical = 'Open AI'",
        [],
    )?;
    connection.execute_batch("PRAGMA user_version = 3;")?;
    drop(connection);

    let store = SqliteStorage::start(path.clone(), secrets)?;
    store.upsert_dictionary(
        NewDictionaryEntry {
            canonical: "OpenAI".to_owned(),
            language: "en".to_owned(),
            origin: DictionaryOrigin::Manual,
        },
        2_000,
    )?;
    let entries = store.list_dictionary()?;
    assert_eq!(2, entries.len());
    assert!(entries.iter().any(|entry| entry.canonical == "Open AI"));
    assert!(entries.iter().any(|entry| entry.canonical == "OpenAI"));
    drop(store);

    let connection = rusqlite::Connection::open(path)?;
    let version: u32 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    assert_eq!(24, version);
    let spaced_key: String = connection.query_row(
        "SELECT canonical_key FROM dictionary_entries WHERE canonical = 'Open AI'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!("open ai", spaced_key);
    Ok(())
}

#[test]
fn v4_dictionary_learning_data_is_migrated_without_mappings()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("saymore.sqlite3");
    let secrets = Arc::new(MemorySecretStore::default());
    drop(SqliteStorage::start(path.clone(), secrets.clone())?);

    let connection = rusqlite::Connection::open(&path)?;
    connection.execute_batch(
        "DROP INDEX term_observations_candidate;
         DROP TABLE term_observations;
         DROP TABLE dictionary_candidates;
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
         UPDATE app_settings SET automatic_dictionary_learning = 0 WHERE singleton = 1;
         INSERT INTO term_observations(
            dictation_id, language, canonical, canonical_key, recognized_as,
            variant_key, occurrence_count, observed_at_ms
         ) VALUES ('legacy-dictation', 'en', 'Saymore', 'saymore', 'say more',
                   'say more', 2, 1000);
         INSERT INTO dictionary_candidates(
            language, canonical_key, variant_key, occurrence_count,
            dictation_count, last_observed_at_ms
         ) VALUES ('en', 'saymore', 'say more', 2, 1, 1000);
         INSERT INTO dictionary_suppressions(
            language, canonical_key, suppressed_until_ms
         ) VALUES ('en', 'saymore', 90000);
         PRAGMA user_version = 4;",
    )?;
    drop(connection);

    let store = SqliteStorage::start(path.clone(), secrets)?;
    let mut settings = store.load_settings()?;
    settings.history_retention = HistoryRetention::ThirtyDays;
    store.save_settings(settings)?;
    drop(store);

    let connection = rusqlite::Connection::open(path)?;
    let legacy_setting: bool = connection.query_row(
        "SELECT automatic_dictionary_learning FROM app_settings WHERE singleton = 1",
        [],
        |row| row.get(0),
    )?;
    assert!(!legacy_setting);
    for table in [
        "term_observations",
        "dictionary_candidates",
        "dictionary_suppressions",
    ] {
        let count: u32 =
            connection.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })?;
        assert_eq!(1, count, "legacy rows in {table} must be preserved");
    }
    let variants_table: u32 = connection.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'dictionary_variants'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(0, variants_table);
    Ok(())
}

#[test]
fn installed_model_metadata_is_upserted_and_deleted_by_stable_id()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let store = SqliteStorage::start(
        directory.path().join("saymore.sqlite3"),
        Arc::new(MemorySecretStore::default()),
    )?;
    let mut model = InstalledModel {
        id: "model-id".to_owned(),
        provider_type: "local-paraformer".to_owned(),
        model: "paraformer".to_owned(),
        version: "1".to_owned(),
        path: "/models/one".to_owned(),
        installed_at_ms: 1_000,
        last_verified_at_ms: None,
    };
    store.save_installed_model(model.clone())?;
    model.last_verified_at_ms = Some(2_000);
    store.save_installed_model(model.clone())?;

    assert_eq!(vec![model], store.list_installed_models()?);
    store.delete_installed_model("model-id")?;
    assert!(store.list_installed_models()?.is_empty());
    Ok(())
}
