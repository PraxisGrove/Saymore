use rusqlite::{Connection, params};
use template_app::{HistoryCursor, HistoryPage, StorageError};

use super::{Database, history};

pub(super) fn page(
    database: &mut Database,
    cursor: Option<HistoryCursor>,
    limit: u16,
    query: &str,
) -> Result<HistoryPage, StorageError> {
    let normalized_query = query.trim().to_lowercase();
    if normalized_query.is_empty() {
        return history::page(database, cursor, limit);
    }
    let key = *history::ensure_key(database)?;
    let limit = usize::from(limit.clamp(1, 50));
    let mut scan_cursor = cursor;
    let mut records = Vec::with_capacity(limit);
    let mut has_more = true;

    'scan: while records.len() < limit && has_more {
        let rows = encrypted_rows_after(&database.connection, scan_cursor.as_ref(), 51)?;
        let database_has_more = rows.len() > 50;
        let rows = rows.into_iter().take(50).collect::<Vec<_>>();
        has_more = database_has_more;
        let batch_len = rows.len();
        for (index, row) in rows.into_iter().enumerate() {
            scan_cursor = Some(HistoryCursor {
                created_at_ms: row.created_at_ms,
                id: row.id.clone(),
            });
            let record = history::decrypt_history(&key, row)?;
            if record.final_text.to_lowercase().contains(&normalized_query) {
                records.push(record);
                if records.len() == limit {
                    has_more = index + 1 < batch_len || database_has_more;
                    break 'scan;
                }
            }
        }
    }

    Ok(HistoryPage {
        records,
        next_cursor: has_more.then_some(scan_cursor).flatten(),
    })
}

fn encrypted_rows_after(
    connection: &Connection,
    cursor: Option<&HistoryCursor>,
    limit: i64,
) -> Result<Vec<history::EncryptedRow>, StorageError> {
    match cursor {
        Some(cursor) => history::query_rows(
            connection,
            "SELECT id, created_at_ms, crypto_version, payload_version, nonce, ciphertext
             FROM transcript_history
             WHERE created_at_ms < ?1 OR (created_at_ms = ?1 AND id < ?2)
             ORDER BY created_at_ms DESC, id DESC LIMIT ?3",
            params![cursor.created_at_ms, cursor.id, limit],
        ),
        None => history::query_rows(
            connection,
            "SELECT id, created_at_ms, crypto_version, payload_version, nonce, ciphertext
             FROM transcript_history
             ORDER BY created_at_ms DESC, id DESC LIMIT ?1",
            params![limit],
        ),
    }
}
