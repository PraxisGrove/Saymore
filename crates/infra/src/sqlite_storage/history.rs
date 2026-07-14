use std::time::{SystemTime, UNIX_EPOCH};

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, AeadCore, KeyInit, OsRng, Payload},
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use template_app::{
    HistoryCursor, HistoryDelivery, HistoryPage, HistoryRecord, HistoryRefinement,
    HistoryRetention, NewHistoryRecord, SecretStore, StorageError,
};

use super::{Database, invalid, settings, unavailable};

const CRYPTO_VERSION: u32 = 1;
const PAYLOAD_VERSION: u32 = 1;
const KEY_BYTES: usize = 32;
const NONCE_BYTES: usize = 12;
const DAY_MS: i64 = 86_400_000;

pub(super) enum HistoryKeyState {
    Uninitialized,
    Available([u8; KEY_BYTES]),
    Locked,
    Invalid(String),
    Unavailable(String),
}

#[derive(Debug, Serialize, Deserialize)]
struct HistoryPayload {
    #[serde(rename = "text")]
    final_text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    raw_asr_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "refined_text")]
    llm_refined_text: Option<String>,
    audio_duration_ms: u64,
    language: Option<String>,
    delivery: String,
    refinement: String,
    asr_provider_id: Option<String>,
    llm_provider_id: Option<String>,
}

struct EncryptedRow {
    id: String,
    created_at_ms: i64,
    crypto_version: u32,
    payload_version: u32,
    nonce: Vec<u8>,
    ciphertext: Vec<u8>,
}

pub(super) fn initialize_key(
    connection: &mut Connection,
    secrets: &dyn SecretStore,
) -> HistoryKeyState {
    let row_count = connection.query_row("SELECT COUNT(*) FROM transcript_history", [], |row| {
        row.get::<_, u64>(0)
    });
    let Ok(row_count) = row_count else {
        return HistoryKeyState::Unavailable("history table could not be read".to_owned());
    };
    match secrets.load_history_key() {
        Ok(Some(bytes)) => match key_array(&bytes) {
            Ok(key) => match validate_or_create_key_check(connection, &key, row_count) {
                Ok(()) => HistoryKeyState::Available(key),
                Err(StorageError::HistoryLocked) => HistoryKeyState::Locked,
                Err(StorageError::Invalid(message)) => HistoryKeyState::Invalid(message),
                Err(error) => HistoryKeyState::Unavailable(error.to_string()),
            },
            Err(_) => HistoryKeyState::Locked,
        },
        Ok(None) if row_count > 0 => HistoryKeyState::Locked,
        Ok(None) => match create_and_store_key(connection, secrets) {
            Ok(key) => HistoryKeyState::Available(key),
            Err(error) => HistoryKeyState::Unavailable(error.to_string()),
        },
        Err(error) => HistoryKeyState::Unavailable(error.to_string()),
    }
}

pub(super) fn insert(
    database: &mut Database,
    record: NewHistoryRecord,
) -> Result<(), StorageError> {
    let local_settings = settings::load(&database.connection)?;
    if !local_settings.history_enabled {
        return Ok(());
    }
    if record.id.trim().is_empty() || record.final_text.trim().is_empty() {
        return Err(StorageError::Invalid(
            "history id and final text must not be empty".to_owned(),
        ));
    }
    let key = *ensure_key(database)?;
    let plaintext = serde_json::to_vec(&HistoryPayload::from_record(&record)).map_err(invalid)?;
    let aad = history_aad(&record.id, record.created_at_ms, PAYLOAD_VERSION);
    let (nonce, ciphertext) = encrypt(&key, &plaintext, &aad)?;
    let transaction = database.connection.transaction().map_err(unavailable)?;
    transaction
        .execute(
            "INSERT OR IGNORE INTO transcript_history(
                id, created_at_ms, crypto_version, payload_version, nonce, ciphertext
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                record.id,
                record.created_at_ms,
                CRYPTO_VERSION,
                PAYLOAD_VERSION,
                nonce,
                ciphertext
            ],
        )
        .map_err(unavailable)?;
    cleanup_in_transaction(
        &transaction,
        record.created_at_ms,
        local_settings.history_retention,
    )?;
    transaction.commit().map_err(unavailable)
}

pub(super) fn page(
    database: &mut Database,
    cursor: Option<HistoryCursor>,
    limit: u16,
) -> Result<HistoryPage, StorageError> {
    let key = *ensure_key(database)?;
    let limit = i64::from(limit.clamp(1, 50));
    let query_limit = limit + 1;
    let rows = match cursor {
        Some(cursor) => query_rows(
            &database.connection,
            "SELECT id, created_at_ms, crypto_version, payload_version, nonce, ciphertext
             FROM transcript_history
             WHERE created_at_ms < ?1 OR (created_at_ms = ?1 AND id < ?2)
             ORDER BY created_at_ms DESC, id DESC LIMIT ?3",
            params![cursor.created_at_ms, cursor.id, query_limit],
        )?,
        None => query_rows(
            &database.connection,
            "SELECT id, created_at_ms, crypto_version, payload_version, nonce, ciphertext
             FROM transcript_history
             ORDER BY created_at_ms DESC, id DESC LIMIT ?1",
            params![query_limit],
        )?,
    };
    let has_more = rows.len() > limit as usize;
    let records = rows
        .into_iter()
        .take(limit as usize)
        .map(|row| decrypt_history(&key, row))
        .collect::<Result<Vec<_>, _>>()?;
    let next_cursor = has_more
        .then(|| {
            records.last().map(|record| HistoryCursor {
                created_at_ms: record.created_at_ms,
                id: record.id.clone(),
            })
        })
        .flatten();
    Ok(HistoryPage {
        records,
        next_cursor,
    })
}

pub(super) fn delete(connection: &mut Connection, id: &str) -> Result<(), StorageError> {
    connection
        .execute("DELETE FROM transcript_history WHERE id = ?1", [id])
        .map_err(unavailable)?;
    checkpoint(connection)
}

pub(super) fn update_delivery(
    database: &mut Database,
    id: &str,
    delivery: HistoryDelivery,
) -> Result<(), StorageError> {
    let key = *ensure_key(database)?;
    let row = database
        .connection
        .query_row(
            "SELECT id, created_at_ms, crypto_version, payload_version, nonce, ciphertext
             FROM transcript_history WHERE id = ?1",
            [id],
            |row| {
                Ok(EncryptedRow {
                    id: row.get(0)?,
                    created_at_ms: row.get(1)?,
                    crypto_version: row.get(2)?,
                    payload_version: row.get(3)?,
                    nonce: row.get(4)?,
                    ciphertext: row.get(5)?,
                })
            },
        )
        .optional()
        .map_err(unavailable)?
        .ok_or_else(|| StorageError::Invalid("history record is missing".to_owned()))?;
    validate_encrypted_row_versions(&row)?;
    let aad = history_aad(&row.id, row.created_at_ms, row.payload_version);
    let plaintext = decrypt(&key, &row.nonce, &row.ciphertext, &aad)?;
    let mut payload: HistoryPayload = serde_json::from_slice(&plaintext).map_err(invalid)?;
    payload.delivery = delivery_name(delivery).to_owned();
    let plaintext = serde_json::to_vec(&payload).map_err(invalid)?;
    let (nonce, ciphertext) = encrypt(&key, &plaintext, &aad)?;
    database
        .connection
        .execute(
            "UPDATE transcript_history SET nonce = ?2, ciphertext = ?3 WHERE id = ?1",
            params![id, nonce, ciphertext],
        )
        .map_err(unavailable)?;
    Ok(())
}

pub(super) fn clear(connection: &mut Connection) -> Result<(), StorageError> {
    connection
        .execute("DELETE FROM transcript_history", [])
        .map_err(unavailable)?;
    checkpoint(connection)
}

pub(super) fn reset(database: &mut Database) -> Result<(), StorageError> {
    let transaction = database.connection.transaction().map_err(unavailable)?;
    transaction
        .execute("DELETE FROM transcript_history", [])
        .map_err(unavailable)?;
    transaction
        .execute("DELETE FROM history_key_validation", [])
        .map_err(unavailable)?;
    transaction.commit().map_err(unavailable)?;
    checkpoint(&mut database.connection)?;
    database.history_key = HistoryKeyState::Unavailable(
        "history key rotation did not complete; restart Saymore to retry".to_owned(),
    );
    database.secrets.delete_history_key().map_err(unavailable)?;
    let generated = Aes256Gcm::generate_key(&mut OsRng);
    let mut key = [0_u8; KEY_BYTES];
    key.copy_from_slice(generated.as_slice());
    database
        .secrets
        .save_history_key(&key)
        .map_err(unavailable)?;
    write_key_check(&mut database.connection, &key)?;
    database.history_key = HistoryKeyState::Available(key);
    Ok(())
}

pub(super) fn cleanup(connection: &mut Connection, now_ms: i64) -> Result<u64, StorageError> {
    let retention = settings::load(connection)?.history_retention;
    let transaction = connection.transaction().map_err(unavailable)?;
    let deleted = cleanup_in_transaction(&transaction, now_ms, retention)?;
    transaction.commit().map_err(unavailable)?;
    if deleted > 0 {
        checkpoint(connection)?;
    }
    Ok(deleted)
}

pub(super) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

fn available_key(state: &HistoryKeyState) -> Result<&[u8; KEY_BYTES], StorageError> {
    match state {
        HistoryKeyState::Uninitialized => Err(StorageError::Unavailable(
            "history key was not initialized".to_owned(),
        )),
        HistoryKeyState::Available(key) => Ok(key),
        HistoryKeyState::Locked => Err(StorageError::HistoryLocked),
        HistoryKeyState::Invalid(message) => Err(StorageError::Invalid(message.clone())),
        HistoryKeyState::Unavailable(message) => Err(StorageError::Unavailable(message.clone())),
    }
}

fn ensure_key(database: &mut Database) -> Result<&[u8; KEY_BYTES], StorageError> {
    if matches!(database.history_key, HistoryKeyState::Uninitialized) {
        database.history_key = initialize_key(&mut database.connection, database.secrets.as_ref());
    }
    available_key(&database.history_key)
}

fn create_and_store_key(
    connection: &mut Connection,
    secrets: &dyn SecretStore,
) -> Result<[u8; KEY_BYTES], StorageError> {
    let generated = Aes256Gcm::generate_key(&mut OsRng);
    let mut key = [0_u8; KEY_BYTES];
    key.copy_from_slice(generated.as_slice());
    secrets.save_history_key(&key).map_err(unavailable)?;
    write_key_check(connection, &key)?;
    Ok(key)
}

fn validate_or_create_key_check(
    connection: &mut Connection,
    key: &[u8; KEY_BYTES],
    row_count: u64,
) -> Result<(), StorageError> {
    let check = connection
        .query_row(
            "SELECT nonce, ciphertext FROM history_key_validation WHERE singleton = 1",
            [],
            |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?)),
        )
        .optional()
        .map_err(unavailable)?;
    if let Some((nonce, ciphertext)) = check {
        let plaintext = match decrypt(key, &nonce, &ciphertext, b"saymore-history-key-v1") {
            Err(StorageError::Invalid(message)) if message == "history authentication failed" => {
                if row_count == 0 {
                    write_key_check(connection, key)?;
                    return Ok(());
                }
                let row = first_row(connection)?
                    .ok_or_else(|| StorageError::Invalid("history row count changed".to_owned()))?;
                return match decrypt_history(key, row) {
                    Ok(_) => Err(StorageError::Invalid(
                        "history key validation record is corrupted".to_owned(),
                    )),
                    Err(StorageError::Invalid(row_message))
                        if row_message == "history authentication failed" =>
                    {
                        Err(StorageError::HistoryLocked)
                    }
                    Err(other) => Err(other),
                };
            }
            result => result?,
        };
        return if plaintext == b"valid" {
            Ok(())
        } else {
            Err(StorageError::HistoryLocked)
        };
    }
    if row_count > 0 {
        let row = first_row(connection)?
            .ok_or_else(|| StorageError::Invalid("history row count changed".to_owned()))?;
        if let Err(error) = decrypt_history(key, row) {
            return match error {
                StorageError::Invalid(message) if message == "history authentication failed" => {
                    Err(StorageError::HistoryLocked)
                }
                other => Err(other),
            };
        }
    }
    write_key_check(connection, key)
}

fn write_key_check(connection: &mut Connection, key: &[u8; KEY_BYTES]) -> Result<(), StorageError> {
    let (nonce, ciphertext) = encrypt(key, b"valid", b"saymore-history-key-v1")?;
    connection
        .execute(
            "INSERT INTO history_key_validation(singleton, nonce, ciphertext)
             VALUES (1, ?1, ?2)
             ON CONFLICT(singleton) DO UPDATE SET nonce = excluded.nonce, ciphertext = excluded.ciphertext",
            params![nonce, ciphertext],
        )
        .map_err(unavailable)?;
    Ok(())
}

fn key_array(bytes: &[u8]) -> Result<[u8; KEY_BYTES], StorageError> {
    bytes
        .try_into()
        .map_err(|_| StorageError::Invalid("history key must contain 32 bytes".to_owned()))
}

fn query_rows<P: rusqlite::Params>(
    connection: &Connection,
    sql: &str,
    params: P,
) -> Result<Vec<EncryptedRow>, StorageError> {
    let mut statement = connection.prepare(sql).map_err(unavailable)?;
    statement
        .query_map(params, |row| {
            Ok(EncryptedRow {
                id: row.get(0)?,
                created_at_ms: row.get(1)?,
                crypto_version: row.get(2)?,
                payload_version: row.get(3)?,
                nonce: row.get(4)?,
                ciphertext: row.get(5)?,
            })
        })
        .map_err(unavailable)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(unavailable)
}

fn first_row(connection: &Connection) -> Result<Option<EncryptedRow>, StorageError> {
    connection
        .query_row(
            "SELECT id, created_at_ms, crypto_version, payload_version, nonce, ciphertext
             FROM transcript_history ORDER BY created_at_ms, id LIMIT 1",
            [],
            |row| {
                Ok(EncryptedRow {
                    id: row.get(0)?,
                    created_at_ms: row.get(1)?,
                    crypto_version: row.get(2)?,
                    payload_version: row.get(3)?,
                    nonce: row.get(4)?,
                    ciphertext: row.get(5)?,
                })
            },
        )
        .optional()
        .map_err(unavailable)
}

fn decrypt_history(
    key: &[u8; KEY_BYTES],
    row: EncryptedRow,
) -> Result<HistoryRecord, StorageError> {
    validate_encrypted_row_versions(&row)?;
    let aad = history_aad(&row.id, row.created_at_ms, row.payload_version);
    let plaintext = decrypt(key, &row.nonce, &row.ciphertext, &aad)?;
    let payload: HistoryPayload = serde_json::from_slice(&plaintext).map_err(invalid)?;
    payload.into_record(row.id, row.created_at_ms)
}

fn validate_encrypted_row_versions(row: &EncryptedRow) -> Result<(), StorageError> {
    if row.crypto_version != CRYPTO_VERSION {
        return Err(StorageError::Invalid(format!(
            "unsupported history crypto version: {}",
            row.crypto_version
        )));
    }
    if row.payload_version != PAYLOAD_VERSION {
        return Err(StorageError::Invalid(format!(
            "unsupported history payload version: {}",
            row.payload_version
        )));
    }
    Ok(())
}

fn cleanup_in_transaction(
    transaction: &rusqlite::Transaction<'_>,
    now_ms: i64,
    retention: HistoryRetention,
) -> Result<u64, StorageError> {
    let Some(days) = retention.days() else {
        return Ok(0);
    };
    let cutoff = now_ms.saturating_sub(i64::from(days) * DAY_MS);
    transaction
        .execute(
            "DELETE FROM transcript_history WHERE created_at_ms < ?1",
            [cutoff],
        )
        .map(|count| count as u64)
        .map_err(unavailable)
}

fn checkpoint(connection: &mut Connection) -> Result<(), StorageError> {
    connection
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
        .map_err(unavailable)
}

fn encrypt(
    key: &[u8; KEY_BYTES],
    plaintext: &[u8],
    aad: &[u8],
) -> Result<(Vec<u8>, Vec<u8>), StorageError> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(invalid)?;
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(
            &nonce,
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| StorageError::Invalid("history encryption failed".to_owned()))?;
    Ok((nonce.to_vec(), ciphertext))
}

fn decrypt(
    key: &[u8; KEY_BYTES],
    nonce: &[u8],
    ciphertext: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>, StorageError> {
    if nonce.len() != NONCE_BYTES {
        return Err(StorageError::Invalid(
            "history nonce has an invalid size".to_owned(),
        ));
    }
    let cipher = Aes256Gcm::new_from_slice(key).map_err(invalid)?;
    cipher
        .decrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|_| StorageError::Invalid("history authentication failed".to_owned()))
}

fn history_aad(id: &str, created_at_ms: i64, payload_version: u32) -> Vec<u8> {
    format!("{id}\0{created_at_ms}\0{payload_version}").into_bytes()
}

impl HistoryPayload {
    fn from_record(record: &NewHistoryRecord) -> Self {
        Self {
            final_text: record.final_text.clone(),
            raw_asr_text: record.raw_asr_text.clone(),
            llm_refined_text: record.llm_refined_text.clone(),
            audio_duration_ms: record.audio_duration_ms,
            language: record.language.clone(),
            delivery: delivery_name(record.delivery).to_owned(),
            refinement: refinement_name(record.refinement).to_owned(),
            asr_provider_id: record.asr_provider_id.clone(),
            llm_provider_id: record.llm_provider_id.clone(),
        }
    }

    fn into_record(self, id: String, created_at_ms: i64) -> Result<HistoryRecord, StorageError> {
        Ok(HistoryRecord {
            id,
            created_at_ms,
            final_text: self.final_text,
            raw_asr_text: self.raw_asr_text,
            llm_refined_text: self.llm_refined_text,
            audio_duration_ms: self.audio_duration_ms,
            language: self.language,
            delivery: parse_delivery(&self.delivery)?,
            refinement: parse_refinement(&self.refinement)?,
            asr_provider_id: self.asr_provider_id,
            llm_provider_id: self.llm_provider_id,
        })
    }
}

fn delivery_name(value: HistoryDelivery) -> &'static str {
    match value {
        HistoryDelivery::Delivered => "delivered",
        HistoryDelivery::NotDelivered => "not_delivered",
    }
}

fn parse_delivery(value: &str) -> Result<HistoryDelivery, StorageError> {
    match value {
        "delivered" => Ok(HistoryDelivery::Delivered),
        "not_delivered" => Ok(HistoryDelivery::NotDelivered),
        other => Err(StorageError::Invalid(format!(
            "unknown delivery state: {other}"
        ))),
    }
}

fn refinement_name(value: HistoryRefinement) -> &'static str {
    match value {
        HistoryRefinement::NotUsed => "not_used",
        HistoryRefinement::Completed => "completed",
        HistoryRefinement::TimedOut => "timed_out",
        HistoryRefinement::ProviderUnavailable => "provider_unavailable",
        HistoryRefinement::OutputRejected => "output_rejected",
    }
}

fn parse_refinement(value: &str) -> Result<HistoryRefinement, StorageError> {
    match value {
        "not_used" => Ok(HistoryRefinement::NotUsed),
        "completed" => Ok(HistoryRefinement::Completed),
        "timed_out" => Ok(HistoryRefinement::TimedOut),
        "provider_unavailable" => Ok(HistoryRefinement::ProviderUnavailable),
        "output_rejected" => Ok(HistoryRefinement::OutputRejected),
        other => Err(StorageError::Invalid(format!(
            "unknown refinement state: {other}"
        ))),
    }
}
