use chrono::{Local, NaiveDate, TimeZone};
use rusqlite::{OptionalExtension, Transaction, params};
use template_app::{
    DailyUsage, HistoryCursor, NewHistoryRecord, StorageError, UsageSnapshot, UsageStore,
};

use super::{Command, Database, SqliteStorage, history, invalid, unavailable};

const HISTORY_PAGE_SIZE: u16 = 50;
const MAX_SQLITE_INTEGER: i64 = i64::MAX;

impl UsageStore for SqliteStorage {
    fn usage_snapshot(
        &self,
        period_start: NaiveDate,
        period_end: NaiveDate,
    ) -> Result<UsageSnapshot, StorageError> {
        self.request(|response| Command::UsageSnapshot {
            period_start,
            period_end,
            response,
        })
    }
}

pub(super) fn record(
    transaction: &Transaction<'_>,
    record: &NewHistoryRecord,
) -> Result<(), StorageError> {
    record_values(
        transaction,
        &record.id,
        record.created_at_ms,
        record.audio_duration_ms,
        non_whitespace_characters(&record.final_text),
    )
}

pub(super) fn snapshot(
    database: &mut Database,
    period_start: NaiveDate,
    period_end: NaiveDate,
) -> Result<UsageSnapshot, StorageError> {
    if period_start > period_end {
        return Err(StorageError::Invalid(
            "usage period start must not follow its end".to_owned(),
        ));
    }
    backfill_history(database)?;
    let mut statement = database
        .connection
        .prepare(
            "SELECT local_date, audio_duration_ms, character_count
             FROM usage_daily ORDER BY local_date",
        )
        .map_err(unavailable)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, u64>(1)?,
                row.get::<_, u64>(2)?,
            ))
        })
        .map_err(unavailable)?;
    let mut snapshot = UsageSnapshot::default();
    for row in rows {
        let (stored_date, audio_duration_ms, character_count) = row.map_err(unavailable)?;
        let date = NaiveDate::parse_from_str(&stored_date, "%Y-%m-%d").map_err(invalid)?;
        snapshot.total_duration_ms = snapshot.total_duration_ms.saturating_add(audio_duration_ms);
        snapshot.total_characters = snapshot.total_characters.saturating_add(character_count);
        if (period_start..=period_end).contains(&date) {
            snapshot.days.push(DailyUsage {
                date,
                audio_duration_ms,
                character_count,
            });
        }
    }
    Ok(snapshot)
}

pub(super) fn backfill_history(database: &mut Database) -> Result<(), StorageError> {
    if history_backfilled(&database.connection)? {
        return Ok(());
    }
    let row_count = database
        .connection
        .query_row("SELECT COUNT(*) FROM transcript_history", [], |row| {
            row.get::<_, u64>(0)
        })
        .map_err(unavailable)?;
    let mut records = Vec::new();
    if row_count > 0 {
        let mut cursor: Option<HistoryCursor> = None;
        loop {
            let page = history::page(database, cursor, HISTORY_PAGE_SIZE)?;
            records.extend(page.records);
            let Some(next_cursor) = page.next_cursor else {
                break;
            };
            cursor = Some(next_cursor);
        }
    }
    let transaction = database.connection.transaction().map_err(unavailable)?;
    for record in records {
        record_values(
            &transaction,
            &record.id,
            record.created_at_ms,
            record.audio_duration_ms,
            non_whitespace_characters(&record.final_text),
        )?;
    }
    transaction
        .execute(
            "UPDATE usage_aggregation_state SET history_backfilled = 1 WHERE singleton = 1",
            [],
        )
        .map_err(unavailable)?;
    transaction.commit().map_err(unavailable)
}

fn record_values(
    transaction: &Transaction<'_>,
    id: &str,
    created_at_ms: i64,
    audio_duration_ms: u64,
    character_count: u64,
) -> Result<(), StorageError> {
    let inserted = transaction
        .execute(
            "INSERT OR IGNORE INTO usage_recorded_dictations(dictation_id) VALUES (?1)",
            [id],
        )
        .map_err(unavailable)?;
    if inserted == 0 {
        return Ok(());
    }
    let local_date = Local
        .timestamp_millis_opt(created_at_ms)
        .single()
        .ok_or_else(|| StorageError::Invalid("usage timestamp is out of range".to_owned()))?
        .date_naive()
        .format("%Y-%m-%d")
        .to_string();
    let audio_duration_ms = i64::try_from(audio_duration_ms)
        .map_err(|_| StorageError::Invalid("usage duration is too large".to_owned()))?;
    let character_count = i64::try_from(character_count)
        .map_err(|_| StorageError::Invalid("usage character count is too large".to_owned()))?;
    transaction
        .execute(
            "INSERT INTO usage_daily(local_date, audio_duration_ms, character_count)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(local_date) DO UPDATE SET
                audio_duration_ms = CASE
                    WHEN usage_daily.audio_duration_ms > ?4 - excluded.audio_duration_ms THEN ?4
                    ELSE usage_daily.audio_duration_ms + excluded.audio_duration_ms
                END,
                character_count = CASE
                    WHEN usage_daily.character_count > ?4 - excluded.character_count THEN ?4
                    ELSE usage_daily.character_count + excluded.character_count
                END",
            params![
                local_date,
                audio_duration_ms,
                character_count,
                MAX_SQLITE_INTEGER
            ],
        )
        .map_err(unavailable)?;
    Ok(())
}

fn history_backfilled(connection: &rusqlite::Connection) -> Result<bool, StorageError> {
    connection
        .query_row(
            "SELECT history_backfilled FROM usage_aggregation_state WHERE singleton = 1",
            [],
            |row| row.get::<_, bool>(0),
        )
        .optional()
        .map(|value| value.unwrap_or(false))
        .map_err(unavailable)
}

fn non_whitespace_characters(text: &str) -> u64 {
    text.chars()
        .filter(|character| !character.is_whitespace())
        .count() as u64
}
