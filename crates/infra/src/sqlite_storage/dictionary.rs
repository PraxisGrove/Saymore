use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension, params};
use template_app::{
    DictionaryEntry, DictionaryOrigin, NewDictionaryEntry, StorageError, dictionary_comparison_key,
    dictionary_variant_key, normalize_language_tag,
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
    let entries = statement
        .query_map([], entry_from_row)
        .map_err(unavailable)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(unavailable)?;
    entries
        .into_iter()
        .map(|mut entry| {
            entry.variants = variants(connection, &entry.id)?;
            Ok(entry)
        })
        .collect()
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
    connection
        .execute("DELETE FROM dictionary_entries WHERE id = ?1", [id])
        .map_err(unavailable)?;
    Ok(())
}

struct NormalizedEntry {
    canonical: String,
    canonical_key: String,
    language: String,
    variants: Vec<(String, String)>,
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
        let mut seen = BTreeSet::new();
        let variants = entry
            .variants
            .into_iter()
            .filter_map(|value| {
                let display = value.trim().to_owned();
                let key = dictionary_variant_key(&display);
                (!key.is_empty()
                    && key != dictionary_variant_key(&canonical)
                    && seen.insert(key.clone()))
                .then_some((display, key))
            })
            .collect();
        Ok(Self {
            canonical,
            canonical_key,
            language,
            variants,
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
    for (display, key) in &entry.variants {
        transaction
            .execute(
                "INSERT OR IGNORE INTO dictionary_variants(
                    id, entry_id, recognized_as, variant_key, created_at_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![Uuid::new_v4().to_string(), id, display, key, now_ms],
            )
            .map_err(unavailable)?;
    }
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
    entry
        .map(|mut entry| {
            entry.variants = variants(connection, &entry.id)?;
            Ok(entry)
        })
        .transpose()
}

fn entry_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DictionaryEntry> {
    let origin: String = row.get(3)?;
    Ok(DictionaryEntry {
        id: row.get(0)?,
        canonical: row.get(1)?,
        language: row.get(2)?,
        variants: Vec::new(),
        origin: if origin == "manual" {
            DictionaryOrigin::Manual
        } else {
            DictionaryOrigin::Automatic
        },
        created_at_ms: row.get(4)?,
        updated_at_ms: row.get(5)?,
    })
}

fn variants(connection: &Connection, entry_id: &str) -> Result<Vec<String>, StorageError> {
    let mut statement = connection
        .prepare(
            "SELECT recognized_as FROM dictionary_variants
             WHERE entry_id = ?1 ORDER BY created_at_ms, rowid",
        )
        .map_err(unavailable)?;
    statement
        .query_map([entry_id], |row| row.get(0))
        .map_err(unavailable)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(unavailable)
}
