use super::{MemorySecretStore, history_record};
use jiff::{civil::date, tz::TimeZone};
use std::sync::Arc;
use template_app::{DailyUsage, HistoryStore, UsageStore};
use template_infra::SqliteStorage;

#[test]
fn usage_survives_history_clear_and_duplicate_inserts_count_once()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let store = SqliteStorage::start(
        directory.path().join("saymore.sqlite3"),
        Arc::new(MemorySecretStore::default()),
    )?;
    let completed_at = date(2026, 8, 3)
        .at(10, 0, 0, 0)
        .to_zoned_datetime(TimeZone::system())?;
    let record = history_record(
        "usage-id",
        completed_at.timestamp().as_millisecond(),
        "你好 Saymore",
    );
    store.insert_history(record.clone())?;
    store.insert_history(record)?;

    let before_clear = store.usage_snapshot(completed_at.date(), completed_at.date())?;
    assert_eq!(1_500, before_clear.total_duration_ms);
    assert_eq!(9, before_clear.total_characters);
    assert_eq!(1, before_clear.days.len());

    store.clear_history()?;

    assert!(store.history_page(None, 50)?.records.is_empty());
    assert_eq!(
        before_clear,
        store.usage_snapshot(completed_at.date(), completed_at.date())?
    );
    Ok(())
}

#[test]
fn v19_history_backfills_usage_once() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("saymore.sqlite3");
    let secrets = Arc::new(MemorySecretStore::default());
    let completed_at = date(2026, 8, 2)
        .at(10, 0, 0, 0)
        .to_zoned_datetime(TimeZone::system())?;
    let store = SqliteStorage::start(path.clone(), secrets.clone())?;
    store.insert_history(history_record(
        "legacy-usage",
        completed_at.timestamp().as_millisecond(),
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
    let first = reopened.usage_snapshot(completed_at.date(), completed_at.date())?;
    let second = reopened.usage_snapshot(completed_at.date(), completed_at.date())?;
    assert_eq!(1_500, first.total_duration_ms);
    assert_eq!(3, first.total_characters);
    assert_eq!(first, second);
    Ok(())
}

#[test]
fn new_history_adds_to_existing_iso_date_rows() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("saymore.sqlite3");
    let secrets = Arc::new(MemorySecretStore::default());
    let store = SqliteStorage::start(path.clone(), secrets.clone())?;
    drop(store);

    let connection = rusqlite::Connection::open(&path)?;
    connection.execute(
        "INSERT INTO usage_daily(local_date, audio_duration_ms, character_count)
         VALUES ('2024-02-29', 250, 4)",
        [],
    )?;
    drop(connection);

    let store = SqliteStorage::start(path, secrets)?;
    let day = date(2024, 2, 29);
    let local_noon = day.at(12, 0, 0, 0).to_zoned_datetime(TimeZone::system())?;
    store.insert_history(history_record(
        "new",
        local_noon.timestamp().as_millisecond(),
        "ab",
    ))?;
    let snapshot = store.usage_snapshot(day, day)?;
    assert_eq!(
        snapshot.days,
        [DailyUsage {
            date: day,
            audio_duration_ms: 1_750,
            character_count: 6
        }]
    );
    Ok(())
}
