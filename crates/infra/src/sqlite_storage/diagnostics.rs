use rusqlite::{Connection, params};
use template_app::StorageError;

use super::{history, unavailable};

const MAX_STORED_EVENTS: u32 = 20_000;

pub(super) fn record(connection: &mut Connection, event: &str) -> Result<(), StorageError> {
    if !is_valid_event(event) {
        return Err(StorageError::Invalid(
            "diagnostic event identifier is invalid".to_owned(),
        ));
    }
    let transaction = connection.transaction().map_err(unavailable)?;
    transaction
        .execute(
            "INSERT INTO diagnostic_events(recorded_at_ms, event) VALUES (?1, ?2)",
            params![history::now_ms(), event],
        )
        .map_err(unavailable)?;
    transaction
        .execute(
            "DELETE FROM diagnostic_events
             WHERE id <= COALESCE(
                (SELECT id FROM diagnostic_events ORDER BY id DESC LIMIT 1 OFFSET ?1), 0
             )",
            [MAX_STORED_EVENTS],
        )
        .map_err(unavailable)?;
    transaction.commit().map_err(unavailable)
}

fn is_valid_event(event: &str) -> bool {
    !event.is_empty()
        && event.len() <= 120
        && event
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

pub(super) fn list(connection: &Connection, limit: u32) -> Result<Vec<String>, StorageError> {
    let mut statement = connection
        .prepare(
            "SELECT event FROM (
                SELECT id, event FROM diagnostic_events ORDER BY id DESC LIMIT ?1
             ) ORDER BY id ASC",
        )
        .map_err(unavailable)?;
    statement
        .query_map([limit], |row| row.get(0))
        .map_err(unavailable)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(unavailable)
}
