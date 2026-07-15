use rusqlite::{Connection, OptionalExtension, params};
use template_app::{
    DictionaryEntry, DictionaryOrigin, NewDictionaryEntry, StorageError, dictionary_comparison_key,
    normalize_language_tag,
};
use uuid::Uuid;

use super::unavailable;

pub(super) fn list(connection: &Connection) -> Result<Vec<DictionaryEntry>, StorageError> {
    let mut statement = connection
        .prepare(
            "SELECT id, canonical, language, origin, created_at_ms, updated_at_ms
             FROM dictionary_entries ORDER BY updated_at_ms DESC, canonical",
        )
        .map_err(unavailable)?;
    statement
        .query_map([], entry_from_row)
        .map_err(unavailable)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(unavailable)
}

pub(super) fn upsert(
    connection: &mut Connection,
    entry: NewDictionaryEntry,
    now_ms: i64,
) -> Result<DictionaryEntry, StorageError> {
    let transaction = connection.transaction().map_err(unavailable)?;
    let id = upsert_in_transaction(&transaction, entry, now_ms)?;
    transaction.commit().map_err(unavailable)?;
    find_by_id(connection, &id)?.ok_or_else(|| {
        StorageError::Invalid("dictionary upsert did not create an entry".to_owned())
    })
}

pub(super) fn delete(connection: &mut Connection, id: &str) -> Result<(), StorageError> {
    let transaction = connection.transaction().map_err(unavailable)?;
    let entry = transaction
        .query_row(
            "SELECT language, canonical_key, origin FROM dictionary_entries WHERE id = ?1",
            [id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()
        .map_err(unavailable)?;
    if let Some((language, canonical_key, origin)) = entry
        && origin == "automatic"
    {
        transaction
            .execute(
                "INSERT INTO dictionary_suppressions(language, canonical_key, suppressed_until_ms)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(language, canonical_key) DO UPDATE SET
                    suppressed_until_ms = excluded.suppressed_until_ms",
                params![language, canonical_key, i64::MAX],
            )
            .map_err(unavailable)?;
    }
    transaction
        .execute("DELETE FROM dictionary_entries WHERE id = ?1", [id])
        .map_err(unavailable)?;
    transaction.commit().map_err(unavailable)
}

struct NormalizedEntry {
    canonical: String,
    canonical_key: String,
    language: String,
    origin: DictionaryOrigin,
}

impl NormalizedEntry {
    fn new(entry: NewDictionaryEntry) -> Result<Self, StorageError> {
        let canonical = entry.canonical.trim().to_owned();
        let canonical_key = dictionary_comparison_key(&canonical);
        if canonical_key.is_empty() {
            return Err(StorageError::Invalid(
                "dictionary canonical spelling must not be empty".to_owned(),
            ));
        }
        let language = normalize_language_tag(&entry.language)?;
        Ok(Self {
            canonical,
            canonical_key,
            language,
            origin: entry.origin,
        })
    }
}

pub(super) fn upsert_in_transaction(
    transaction: &rusqlite::Transaction<'_>,
    entry: NewDictionaryEntry,
    now_ms: i64,
) -> Result<String, StorageError> {
    let entry = NormalizedEntry::new(entry)?;
    if entry.origin == DictionaryOrigin::Manual {
        transaction
            .execute(
                "DELETE FROM dictionary_suppressions WHERE language = ?1 AND canonical_key = ?2",
                params![entry.language, entry.canonical_key],
            )
            .map_err(unavailable)?;
    }
    let existing = transaction
        .query_row(
            "SELECT id, origin FROM dictionary_entries
             WHERE language = ?1 AND canonical_key = ?2",
            params![entry.language, entry.canonical_key],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(unavailable)?;
    let id = existing
        .as_ref()
        .map(|(id, _)| id.clone())
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let origin = match (
        existing.as_ref().map(|(_, origin)| origin.as_str()),
        entry.origin,
    ) {
        (Some("manual"), _) | (_, DictionaryOrigin::Manual) => "manual",
        (Some("automatic") | None, DictionaryOrigin::Automatic) => "automatic",
        (Some(other), _) => {
            return Err(StorageError::Invalid(format!(
                "unknown dictionary origin: {other}"
            )));
        }
    };
    transaction
        .execute(
            "INSERT INTO dictionary_entries(
                id, canonical, canonical_key, language, origin, created_at_ms, updated_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
             ON CONFLICT(language, canonical_key) DO UPDATE SET
                canonical = excluded.canonical,
                origin = ?5,
                updated_at_ms = excluded.updated_at_ms",
            params![
                id,
                entry.canonical,
                entry.canonical_key,
                entry.language,
                origin,
                now_ms
            ],
        )
        .map_err(unavailable)?;
    Ok(id)
}

pub(super) fn find_by_id(
    connection: &Connection,
    id: &str,
) -> Result<Option<DictionaryEntry>, StorageError> {
    let entry = connection
        .query_row(
            "SELECT id, canonical, language, origin, created_at_ms, updated_at_ms
             FROM dictionary_entries WHERE id = ?1",
            [id],
            entry_from_row,
        )
        .optional()
        .map_err(unavailable)?;
    Ok(entry)
}

fn entry_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DictionaryEntry> {
    let origin: String = row.get(3)?;
    Ok(DictionaryEntry {
        id: row.get(0)?,
        canonical: row.get(1)?,
        language: row.get(2)?,
        origin: if origin == "manual" {
            DictionaryOrigin::Manual
        } else {
            DictionaryOrigin::Automatic
        },
        created_at_ms: row.get(4)?,
        updated_at_ms: row.get(5)?,
    })
}
