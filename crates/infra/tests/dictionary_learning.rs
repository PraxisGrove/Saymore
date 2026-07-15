#![allow(clippy::panic_in_result_fn)]

use std::sync::{Arc, Mutex};

use template_app::{
    DictionaryCorrection, DictionaryLearningOutcome, DictionaryLearningStore, DictionaryOrigin,
    DictionaryStore, NewDictionaryObservation, SecretStore, SecretStoreError,
};
use template_infra::SqliteStorage;

#[derive(Default)]
struct MemorySecretStore {
    key: Mutex<Option<Vec<u8>>>,
}

impl SecretStore for MemorySecretStore {
    fn load_history_key(&self) -> Result<Option<Vec<u8>>, SecretStoreError> {
        self.key
            .lock()
            .map(|key| key.clone())
            .map_err(|_| SecretStoreError::Unavailable("test key lock poisoned".to_owned()))
    }

    fn save_history_key(&self, key: &[u8]) -> Result<(), SecretStoreError> {
        self.key
            .lock()
            .map(|mut stored| *stored = Some(key.to_vec()))
            .map_err(|_| SecretStoreError::Unavailable("test key lock poisoned".to_owned()))
    }

    fn delete_history_key(&self) -> Result<(), SecretStoreError> {
        self.key
            .lock()
            .map(|mut stored| *stored = None)
            .map_err(|_| SecretStoreError::Unavailable("test key lock poisoned".to_owned()))
    }
}

#[test]
fn repeated_corrections_across_dictations_add_an_automatic_entry()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let store = SqliteStorage::start(
        directory.path().join("saymore.sqlite3"),
        Arc::new(MemorySecretStore::default()),
    )?;

    assert_eq!(
        DictionaryLearningOutcome::Pending {
            occurrence_count: 1,
            dictation_count: 1,
        },
        store.record_dictionary_observation(observation("first", 1_000))?
    );
    assert_eq!(
        DictionaryLearningOutcome::Pending {
            occurrence_count: 2,
            dictation_count: 2,
        },
        store.record_dictionary_observation(observation("second", 2_000))?
    );
    let DictionaryLearningOutcome::Added(added) =
        store.record_dictionary_observation(observation("second", 3_000))?
    else {
        return Err("third observation did not add a dictionary entry".into());
    };
    assert_eq!("Saymore", added.canonical);
    assert_eq!(vec!["CM"], added.variants);
    assert_eq!(DictionaryOrigin::Automatic, added.origin);
    assert_eq!(vec![added], store.list_dictionary()?);
    Ok(())
}

fn observation(dictation_id: &str, observed_at_ms: i64) -> NewDictionaryObservation {
    NewDictionaryObservation {
        dictation_id: dictation_id.to_owned(),
        language: "und".to_owned(),
        correction: DictionaryCorrection {
            recognized_as: "CM".to_owned(),
            canonical: "Saymore".to_owned(),
        },
        observed_at_ms,
    }
}
