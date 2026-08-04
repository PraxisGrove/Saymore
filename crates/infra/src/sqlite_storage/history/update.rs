use rusqlite::{OptionalExtension, params};
use template_app::{HistoryDelivery, StorageError};

use super::{
    Database, EncryptedRow, HistoryPayload, decrypt, delivery_name, encrypt, ensure_key,
    history_aad, invalid, unavailable, validate_encrypted_row_versions,
};

pub(super) fn update_final_text(
    database: &mut Database,
    id: &str,
    final_text: &str,
) -> Result<(), StorageError> {
    update_payload(database, id, |payload| {
        payload.final_text = final_text.to_owned();
    })
}

pub(super) fn update_delivery(
    database: &mut Database,
    id: &str,
    delivery: HistoryDelivery,
) -> Result<(), StorageError> {
    update_payload(database, id, |payload| {
        payload.delivery = delivery_name(delivery).to_owned();
    })
}

fn update_payload(
    database: &mut Database,
    id: &str,
    update: impl FnOnce(&mut HistoryPayload),
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
    update(&mut payload);
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
