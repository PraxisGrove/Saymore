use std::collections::BTreeMap;

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SaymoreSettings {
    pub asr: AsrSettings,
    pub llm: LlmSettings,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AsrSettings {
    pub volcengine: VolcengineAsrSettings,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VolcengineAsrSettings {
    pub enabled: bool,
    pub api_key: String,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmSettings {
    pub enabled: bool,
    pub chat_completions: ChatCompletionsLlmSettings,
}

impl Default for LlmSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            chat_completions: ChatCompletionsLlmSettings::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChatCompletionsLlmSettings {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub custom_headers: BTreeMap<String, String>,
}

#[derive(Debug, PartialEq, Eq, Error)]
pub enum SettingsStoreError {
    #[error("settings storage is unavailable: {0}")]
    Unavailable(String),
    #[error("settings data is invalid: {0}")]
    Invalid(String),
}

/// Loads and atomically saves non-secret and provider configuration.
///
/// Implementations must restrict local file access to the current user. Callers
/// should use a platform secret store instead when a provider requires stronger
/// protection than a user-owned configuration file.
pub trait SettingsStore {
    fn load(&self) -> Result<SaymoreSettings, SettingsStoreError>;

    fn save(&self, settings: &SaymoreSettings) -> Result<(), SettingsStoreError>;
}
