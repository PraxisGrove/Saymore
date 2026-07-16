#![allow(clippy::panic_in_result_fn)]

use std::sync::{Arc, Mutex};

use template_app::{
    CandidateAssessmentSource, CandidateDecision, DictionaryCandidateAssessment,
    DictionaryCandidateKind, DictionaryCorrection, DictionaryLearningOutcome,
    DictionaryLearningStore, DictionaryOrigin, DictionaryStore, NewDictionaryEntry,
    NewDictionaryObservation, SecretStore, SecretStoreError, assess_dictionary_candidate,
    correction_from_edit,
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
fn two_high_confidence_corrections_add_an_automatic_entry() -> Result<(), Box<dyn std::error::Error>>
{
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
    let DictionaryLearningOutcome::Added(added) =
        store.record_dictionary_observation(observation("second", 2_000))?
    else {
        return Err("second high-confidence observation did not add a dictionary entry".into());
    };
    assert_eq!("Saymore", added.canonical);
    assert_eq!(DictionaryOrigin::Automatic, added.origin);
    assert_eq!(vec![added], store.list_dictionary()?);
    Ok(())
}

#[test]
fn repeated_edits_in_one_dictation_do_not_satisfy_the_high_confidence_threshold()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let store = SqliteStorage::start(
        directory.path().join("saymore.sqlite3"),
        Arc::new(MemorySecretStore::default()),
    )?;

    store.record_dictionary_observation(observation("same", 1_000))?;
    assert_eq!(
        DictionaryLearningOutcome::Pending {
            occurrence_count: 2,
            dictation_count: 1,
        },
        store.record_dictionary_observation(observation("same", 2_000))?
    );
    assert!(store.list_dictionary()?.is_empty());
    Ok(())
}

#[test]
fn different_recognized_forms_save_only_the_canonical_entry()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let store = SqliteStorage::start(
        directory.path().join("saymore.sqlite3"),
        Arc::new(MemorySecretStore::default()),
    )?;
    for (dictation_id, original, observed_at_ms) in [
        ("first", "我们使用 CMO 开发", 1_000),
        ("second", "我们使用 C末 开发", 2_000),
    ] {
        let correction = correction_from_edit(original, "我们使用 Saymore 开发")
            .ok_or("the local correction was not recognized")?;
        let outcome = store.record_dictionary_observation(NewDictionaryObservation {
            dictation_id: dictation_id.to_owned(),
            language: "und".to_owned(),
            correction,
            assessment: assess_dictionary_candidate("Saymore"),
            observed_at_ms,
        })?;
        if dictation_id == "first" {
            assert!(matches!(outcome, DictionaryLearningOutcome::Pending { .. }));
        } else {
            assert!(matches!(outcome, DictionaryLearningOutcome::Added(_)));
        }
    }
    let entries = store.list_dictionary()?;
    assert_eq!(1, entries.len());
    assert_eq!("Saymore", entries[0].canonical);
    Ok(())
}

#[test]
fn deleting_an_automatic_entry_suppresses_relearning_until_manual_add()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let store = SqliteStorage::start(
        directory.path().join("saymore.sqlite3"),
        Arc::new(MemorySecretStore::default()),
    )?;
    store.record_dictionary_observation(observation("first", 1_000))?;
    let DictionaryLearningOutcome::Added(added) =
        store.record_dictionary_observation(observation("second", 2_000))?
    else {
        return Err("second high-confidence observation did not add a dictionary entry".into());
    };

    store.delete_dictionary(&added.id)?;
    assert_eq!(
        DictionaryLearningOutcome::Suppressed,
        store.record_dictionary_observation(observation("third", 4_000))?
    );

    let manual = store.upsert_dictionary(
        NewDictionaryEntry {
            canonical: "Saymore".to_owned(),
            language: "und".to_owned(),
            origin: DictionaryOrigin::Manual,
        },
        5_000,
    )?;
    assert_eq!(DictionaryOrigin::Manual, manual.origin);
    store.delete_dictionary(&manual.id)?;
    assert_eq!(
        DictionaryLearningOutcome::Pending {
            occurrence_count: 1,
            dictation_count: 1,
        },
        store.record_dictionary_observation(observation("fourth", 6_000))?
    );
    Ok(())
}

#[test]
fn professional_chinese_terms_need_more_local_evidence_and_keep_an_audit_record()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let store = SqliteStorage::start(
        directory.path().join("saymore.sqlite3"),
        Arc::new(MemorySecretStore::default()),
    )?;
    let terms = ["地理编码", "逆地理编码"];
    for term in terms {
        let mut outcome = DictionaryLearningOutcome::Rejected;
        for (index, dictation) in ["one", "two", "three", "three", "three"].iter().enumerate() {
            outcome = store.record_dictionary_observation(NewDictionaryObservation {
                dictation_id: format!("{term}-{dictation}"),
                language: "zh-Hans".to_owned(),
                correction: DictionaryCorrection {
                    canonical: term.to_owned(),
                },
                assessment: assess_dictionary_candidate(term),
                observed_at_ms: 1_000 + index as i64,
            })?;
        }
        assert!(matches!(outcome, DictionaryLearningOutcome::Added(_)));
    }
    let evidence = store.list_dictionary_candidate_evidence()?;
    assert_eq!(2, evidence.len());
    assert!(evidence.iter().all(|item| item.occurrence_count == 5));
    assert!(evidence.iter().all(|item| item.dictation_count == 3));
    Ok(())
}

#[test]
fn representative_english_terms_are_promoted_to_automatic_entries()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let store = SqliteStorage::start(
        directory.path().join("saymore.sqlite3"),
        Arc::new(MemorySecretStore::default()),
    )?;
    for term in ["Versa", "Vercel", "POI", "immersiveLayoutHeight"] {
        let first = term_observation(term, format!("{term}-one"), 10_000);
        assert!(matches!(
            store.record_dictionary_observation(first)?,
            DictionaryLearningOutcome::Pending { .. }
        ));
        let second = term_observation(term, format!("{term}-two"), 11_000);
        let outcome = store.record_dictionary_observation(second)?;
        assert!(matches!(outcome, DictionaryLearningOutcome::Added(_)));
    }
    let entries = store.list_dictionary()?;
    assert_eq!(4, entries.len());
    assert!(
        entries
            .iter()
            .all(|entry| entry.origin == DictionaryOrigin::Automatic)
    );
    Ok(())
}

#[test]
fn llm_approved_chinese_term_uses_the_high_confidence_threshold()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let store = SqliteStorage::start(
        directory.path().join("saymore.sqlite3"),
        Arc::new(MemorySecretStore::default()),
    )?;
    let assessment = DictionaryCandidateAssessment {
        decision: CandidateDecision::Accept,
        kind: DictionaryCandidateKind::ProfessionalPhrase,
        confidence: 96,
        source: CandidateAssessmentSource::Llm,
    };
    let make_observation = |dictation_id: &str, observed_at_ms| NewDictionaryObservation {
        dictation_id: dictation_id.to_owned(),
        language: "zh-Hans".to_owned(),
        correction: DictionaryCorrection {
            canonical: "逆地理编码".to_owned(),
        },
        assessment,
        observed_at_ms,
    };
    assert!(matches!(
        store.record_dictionary_observation(make_observation("one", 20_000))?,
        DictionaryLearningOutcome::Pending { .. }
    ));
    let outcome = store.record_dictionary_observation(make_observation("two", 21_000))?;
    assert!(matches!(outcome, DictionaryLearningOutcome::Added(_)));
    let evidence = store.list_dictionary_candidate_evidence()?;
    assert_eq!(
        CandidateAssessmentSource::Llm,
        evidence[0].assessment.source
    );
    Ok(())
}

#[test]
fn an_ordinary_sentence_fragment_is_never_accumulated() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let store = SqliteStorage::start(
        directory.path().join("saymore.sqlite3"),
        Arc::new(MemorySecretStore::default()),
    )?;
    let assessment = assess_dictionary_candidate("要求后续变更");
    assert_eq!(CandidateDecision::Reject, assessment.decision);
    assert_eq!(
        DictionaryLearningOutcome::Rejected,
        store.record_dictionary_observation(NewDictionaryObservation {
            dictation_id: "one".to_owned(),
            language: "zh-Hans".to_owned(),
            correction: DictionaryCorrection {
                canonical: "要求后续变更".to_owned(),
            },
            assessment,
            observed_at_ms: 1_000,
        })?
    );
    assert!(store.list_dictionary_candidate_evidence()?.is_empty());
    Ok(())
}

fn observation(dictation_id: &str, observed_at_ms: i64) -> NewDictionaryObservation {
    NewDictionaryObservation {
        dictation_id: dictation_id.to_owned(),
        language: "und".to_owned(),
        correction: DictionaryCorrection {
            canonical: "Saymore".to_owned(),
        },
        assessment: assess_dictionary_candidate("Saymore"),
        observed_at_ms,
    }
}

fn term_observation(
    term: &str,
    dictation_id: String,
    observed_at_ms: i64,
) -> NewDictionaryObservation {
    NewDictionaryObservation {
        dictation_id,
        language: "en".to_owned(),
        correction: DictionaryCorrection {
            canonical: term.to_owned(),
        },
        assessment: assess_dictionary_candidate(term),
        observed_at_ms,
    }
}
