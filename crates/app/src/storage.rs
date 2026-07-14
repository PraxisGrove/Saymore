use thiserror::Error;
use unicode_normalization::UnicodeNormalization;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryRetention {
    OneDay,
    SevenDays,
    ThirtyDays,
    Forever,
}

impl HistoryRetention {
    pub fn days(self) -> Option<u16> {
        match self {
            Self::OneDay => Some(1),
            Self::SevenDays => Some(7),
            Self::ThirtyDays => Some(30),
            Self::Forever => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalSettings {
    pub history_enabled: bool,
    pub history_retention: HistoryRetention,
}

impl Default for LocalSettings {
    fn default() -> Self {
        Self {
            history_enabled: true,
            history_retention: HistoryRetention::SevenDays,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryDelivery {
    Delivered,
    NotDelivered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryRefinement {
    NotUsed,
    Completed,
    TimedOut,
    ProviderUnavailable,
    OutputRejected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewHistoryRecord {
    pub id: String,
    pub created_at_ms: i64,
    pub final_text: String,
    pub raw_asr_text: Option<String>,
    pub llm_refined_text: Option<String>,
    pub audio_duration_ms: u64,
    pub language: Option<String>,
    pub delivery: HistoryDelivery,
    pub refinement: HistoryRefinement,
    pub asr_provider_id: Option<String>,
    pub llm_provider_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryRecord {
    pub id: String,
    pub created_at_ms: i64,
    pub final_text: String,
    pub raw_asr_text: Option<String>,
    pub llm_refined_text: Option<String>,
    pub audio_duration_ms: u64,
    pub language: Option<String>,
    pub delivery: HistoryDelivery,
    pub refinement: HistoryRefinement,
    pub asr_provider_id: Option<String>,
    pub llm_provider_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryCursor {
    pub created_at_ms: i64,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryPage {
    pub records: Vec<HistoryRecord>,
    pub next_cursor: Option<HistoryCursor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DictionaryOrigin {
    Manual,
    Automatic,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewDictionaryEntry {
    pub canonical: String,
    pub language: String,
    pub variants: Vec<String>,
    pub origin: DictionaryOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictionaryEntry {
    pub id: String,
    pub canonical: String,
    pub language: String,
    pub variants: Vec<String>,
    pub origin: DictionaryOrigin,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledModel {
    pub id: String,
    pub provider_type: String,
    pub model: String,
    pub version: String,
    pub path: String,
    pub installed_at_ms: i64,
    pub last_verified_at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SecretStoreError {
    #[error("secret storage is temporarily unavailable: {0}")]
    Unavailable(String),
    #[error("secret storage rejected the data: {0}")]
    Invalid(String),
}

/// Stores the local history data key in an operating-system credential vault.
///
/// Implementations must distinguish a missing key from a temporarily unavailable
/// credential service and must never persist the key in the SQLite database.
pub trait SecretStore: Send + Sync {
    fn load_history_key(&self) -> Result<Option<Vec<u8>>, SecretStoreError>;
    fn save_history_key(&self, key: &[u8]) -> Result<(), SecretStoreError>;
    fn delete_history_key(&self) -> Result<(), SecretStoreError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum StorageError {
    #[error("local storage is unavailable: {0}")]
    Unavailable(String),
    #[error("local storage contains invalid data: {0}")]
    Invalid(String),
    #[error("history is locked because its data key is unavailable")]
    HistoryLocked,
    #[error("the database was created by a newer Saymore version ({0})")]
    NewerSchema(u32),
}

/// Loads and saves the small set of product settings backed by SQLite.
pub trait LocalSettingsStore: Send + Sync {
    fn load_settings(&self) -> Result<LocalSettings, StorageError>;
    fn save_settings(&self, settings: LocalSettings) -> Result<(), StorageError>;
}

/// Persists encrypted final-output history and applies its retention policy.
pub trait HistoryStore: Send + Sync {
    fn insert_history(&self, record: NewHistoryRecord) -> Result<(), StorageError>;
    fn history_page(
        &self,
        cursor: Option<HistoryCursor>,
        limit: u16,
    ) -> Result<HistoryPage, StorageError>;
    fn update_history_delivery(
        &self,
        id: &str,
        delivery: HistoryDelivery,
    ) -> Result<(), StorageError>;
    fn delete_history(&self, id: &str) -> Result<(), StorageError>;
    fn clear_history(&self) -> Result<(), StorageError>;
    fn reset_history(&self) -> Result<(), StorageError>;
    fn cleanup_history(&self, now_ms: i64) -> Result<u64, StorageError>;
}

/// Maintains the user's confirmed dictionary entries.
///
/// Implementations must preserve compatible legacy entry data while exposing only
/// explicit user-driven list, upsert, and delete operations.
pub trait DictionaryStore: Send + Sync {
    fn list_dictionary(&self) -> Result<Vec<DictionaryEntry>, StorageError>;
    fn upsert_dictionary(
        &self,
        entry: NewDictionaryEntry,
        now_ms: i64,
    ) -> Result<DictionaryEntry, StorageError>;
    fn delete_dictionary(&self, id: &str) -> Result<(), StorageError>;
}

/// Stores metadata for models already installed by a trusted runtime flow.
pub trait InstalledModelStore: Send + Sync {
    fn list_installed_models(&self) -> Result<Vec<InstalledModel>, StorageError>;
    fn save_installed_model(&self, model: InstalledModel) -> Result<(), StorageError>;
}

pub fn dictionary_comparison_key(value: &str) -> String {
    value
        .nfkc()
        .flat_map(char::to_lowercase)
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn dictionary_variant_key(value: &str) -> String {
    value
        .nfkc()
        .flat_map(char::to_lowercase)
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn normalize_language_tag(value: &str) -> Result<String, StorageError> {
    let subtags = value.trim().split('-').collect::<Vec<_>>();
    if subtags.is_empty()
        || !(2..=8).contains(&subtags[0].len())
        || !subtags[0]
            .chars()
            .all(|character| character.is_ascii_alphabetic())
        || subtags.iter().any(|subtag| {
            subtag.is_empty()
                || subtag.len() > 8
                || !subtag
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric())
        })
    {
        return Err(StorageError::Invalid(format!(
            "invalid BCP 47 language tag: {value}"
        )));
    }
    Ok(subtags
        .into_iter()
        .enumerate()
        .map(|(index, subtag)| {
            if index == 0 {
                subtag.to_ascii_lowercase()
            } else if subtag.len() == 4 && subtag.chars().all(|value| value.is_ascii_alphabetic()) {
                let mut characters = subtag.chars();
                characters
                    .next()
                    .map(|first| {
                        format!(
                            "{}{}",
                            first.to_ascii_uppercase(),
                            characters.as_str().to_ascii_lowercase()
                        )
                    })
                    .unwrap_or_default()
            } else if subtag.len() == 2 && subtag.chars().all(|value| value.is_ascii_alphabetic()) {
                subtag.to_ascii_uppercase()
            } else {
                subtag.to_ascii_lowercase()
            }
        })
        .collect::<Vec<_>>()
        .join("-"))
}
