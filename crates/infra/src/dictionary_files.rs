use std::{collections::BTreeMap, path::Path, sync::Arc};

use template_app::{
    DictionaryEntry, DictionaryOrigin, DictionaryStore, NewDictionaryEntry, StorageError,
    dictionary_comparison_key, normalize_language_tag,
};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DictionaryFileReport {
    pub added: usize,
    pub skipped: usize,
}

#[derive(Debug, Error)]
pub enum DictionaryFileError {
    #[error("dictionary CSV is invalid: {0}")]
    Csv(String),
    #[error(transparent)]
    Storage(#[from] StorageError),
}

pub struct DictionaryFiles {
    store: Arc<dyn DictionaryStore>,
}

impl DictionaryFiles {
    pub fn new(store: Arc<dyn DictionaryStore>) -> Self {
        Self { store }
    }

    pub fn import_csv(
        &self,
        path: &Path,
        default_language: &str,
        now_ms: i64,
    ) -> Result<DictionaryFileReport, DictionaryFileError> {
        let default_language = normalize_language_tag(default_language)?;
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .trim(csv::Trim::All)
            .from_path(path)
            .map_err(csv_error)?;
        let mut rows = Vec::new();
        for (index, record) in reader.records().enumerate() {
            let record = record.map_err(csv_error)?;
            if record.len() > 2 {
                return Err(DictionaryFileError::Csv(
                    "expected at most two columns: term and optional language".to_owned(),
                ));
            }
            if index == 0 && is_header(&record) {
                continue;
            }
            let term = record.get(0).unwrap_or_default().trim().to_owned();
            if term.is_empty() {
                continue;
            }
            let language = match record
                .get(1)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                Some(value) => normalize_language_tag(value)?,
                None => default_language.clone(),
            };
            rows.push((term, language));
        }
        self.import_terms(rows, now_ms)
    }

    fn import_terms(
        &self,
        terms: Vec<(String, String)>,
        now_ms: i64,
    ) -> Result<DictionaryFileReport, DictionaryFileError> {
        let existing = self.store.list_dictionary()?;
        let mut identities = existing
            .iter()
            .map(|entry| (identity(entry), entry.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut report = DictionaryFileReport::default();
        for (canonical, language) in terms {
            let key = identity_parts(&canonical, &language);
            if identities.contains_key(&key) {
                report.skipped += 1;
                continue;
            }
            let inserted = self.store.upsert_dictionary(
                NewDictionaryEntry {
                    canonical,
                    language,
                    variants: Vec::new(),
                    origin: DictionaryOrigin::Manual,
                },
                now_ms,
            )?;
            identities.insert(key, inserted);
            report.added += 1;
        }
        Ok(report)
    }
}

fn identity(entry: &DictionaryEntry) -> String {
    identity_parts(&entry.canonical, &entry.language)
}

fn identity_parts(canonical: &str, language: &str) -> String {
    let canonical = dictionary_comparison_key(canonical);
    format!("{language}\0{canonical}")
}

fn is_header(record: &csv::StringRecord) -> bool {
    matches!(
        record
            .get(0)
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "term" | "word" | "canonical" | "词汇" | "词条"
    )
}

fn csv_error(error: csv::Error) -> DictionaryFileError {
    DictionaryFileError::Csv(error.to_string())
}
