#![allow(clippy::panic_in_result_fn)]

use std::sync::{Arc, Mutex};

use template_app::{
    DictionaryOrigin, DictionaryStore, NewDictionaryEntry, SecretStore, SecretStoreError,
};
use template_infra::{DictionaryFiles, SqliteStorage};

#[derive(Default)]
struct MemorySecretStore(Mutex<Option<Vec<u8>>>);

impl SecretStore for MemorySecretStore {
    fn load_history_key(&self) -> Result<Option<Vec<u8>>, SecretStoreError> {
        self.0
            .lock()
            .map(|key| key.clone())
            .map_err(|_| SecretStoreError::Unavailable("test secret lock poisoned".to_owned()))
    }

    fn save_history_key(&self, key: &[u8]) -> Result<(), SecretStoreError> {
        self.0
            .lock()
            .map(|mut stored| *stored = Some(key.to_vec()))
            .map_err(|_| SecretStoreError::Unavailable("test secret lock poisoned".to_owned()))
    }

    fn delete_history_key(&self) -> Result<(), SecretStoreError> {
        self.0
            .lock()
            .map(|mut stored| *stored = None)
            .map_err(|_| SecretStoreError::Unavailable("test secret lock poisoned".to_owned()))
    }
}

#[test]
fn csv_import_accepts_standard_spellings_with_optional_languages()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let store = Arc::new(SqliteStorage::start(
        directory.path().join("saymore.sqlite3"),
        Arc::new(MemorySecretStore::default()),
    )?);
    let csv = directory.path().join("dictionary.csv");
    std::fs::write(&csv, "term,language\nSaymore,en\nSQLite,en\nFigma,\n")?;
    let report = DictionaryFiles::new(store.clone()).import_csv(&csv, "zh-Hans", 1_000)?;

    assert_eq!(3, report.added);
    let entries = store.list_dictionary()?;
    assert_eq!(3, entries.len());
    assert!(
        entries
            .iter()
            .all(|entry| entry.origin == DictionaryOrigin::Manual && entry.variants.is_empty())
    );
    assert!(
        entries
            .iter()
            .any(|entry| { entry.canonical == "Figma" && entry.language == "zh-Hans" })
    );
    Ok(())
}

#[test]
fn csv_duplicate_preserves_the_existing_manual_spelling() -> Result<(), Box<dyn std::error::Error>>
{
    let directory = tempfile::tempdir()?;
    let store = Arc::new(SqliteStorage::start(
        directory.path().join("saymore.sqlite3"),
        Arc::new(MemorySecretStore::default()),
    )?);
    store.upsert_dictionary(
        NewDictionaryEntry {
            canonical: "OpenAI".to_owned(),
            language: "en".to_owned(),
            variants: Vec::new(),
            origin: DictionaryOrigin::Manual,
        },
        1_000,
    )?;
    let csv = directory.path().join("dictionary.csv");
    std::fs::write(&csv, "openai,en\n")?;

    let report = DictionaryFiles::new(store.clone()).import_csv(&csv, "zh-Hans", 2_000)?;

    assert_eq!(0, report.added);
    assert_eq!(1, report.skipped);
    let entries = store.list_dictionary()?;
    assert_eq!(1, entries.len());
    assert_eq!("OpenAI", entries[0].canonical);
    assert!(entries[0].variants.is_empty());
    Ok(())
}

#[test]
fn csv_import_rejects_legacy_variant_columns() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let store = Arc::new(SqliteStorage::start(
        directory.path().join("saymore.sqlite3"),
        Arc::new(MemorySecretStore::default()),
    )?);
    let csv = directory.path().join("dictionary.csv");
    std::fs::write(&csv, "term,language,variants\nSaymore,en,赛摩\n")?;

    let result = DictionaryFiles::new(store).import_csv(&csv, "zh-Hans", 1_000);

    assert!(result.is_err());
    Ok(())
}
