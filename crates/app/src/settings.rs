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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LlmSettings {
    pub enabled: bool,
    pub confirmed_base_url: String,
    pub chat_completions: ChatCompletionsLlmSettings,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChatCompletionsLlmSettings {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub custom_headers: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmProviderPreset {
    SenseNova,
    DeepSeek,
}

impl LlmProviderPreset {
    pub const fn label(self) -> &'static str {
        match self {
            Self::SenseNova => "商汤 SenseNova",
            Self::DeepSeek => "DeepSeek",
        }
    }

    pub const fn id(self) -> &'static str {
        match self {
            Self::SenseNova => "sensenova",
            Self::DeepSeek => "deepseek",
        }
    }

    pub const fn base_url(self) -> &'static str {
        match self {
            Self::SenseNova => "https://token.sensenova.cn/v1",
            Self::DeepSeek => "https://api.deepseek.com",
        }
    }

    pub const fn model(self) -> &'static str {
        match self {
            Self::SenseNova => "sensenova-6.7-flash-lite",
            Self::DeepSeek => "deepseek-v4-flash",
        }
    }

    pub const fn model_list_url(self) -> &'static str {
        match self {
            Self::SenseNova => "https://api.sensenova.cn/v1/llm/models",
            Self::DeepSeek => "https://api.deepseek.com/models",
        }
    }

    pub fn settings(self, api_key: &str) -> ChatCompletionsLlmSettings {
        ChatCompletionsLlmSettings {
            base_url: self.base_url().to_owned(),
            api_key: api_key.trim().to_owned(),
            model: self.model().to_owned(),
            custom_headers: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ActiveProviders {
    pub asr: Option<String>,
    pub llm: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderDataConsent {
    pub fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderInstance {
    pub id: String,
    pub name: String,
    pub provider_type: String,
    pub config: serde_json::Value,
    pub data_consent: Option<ProviderDataConsent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProviderCatalog {
    pub active: ActiveProviders,
    pub asr_providers: Vec<ProviderInstance>,
    pub llm_providers: Vec<ProviderInstance>,
}

impl ProviderCatalog {
    pub fn save_llm_provider_config(&mut self, preset: LlmProviderPreset, api_key: &str) {
        self.save_llm_provider_model_config(preset, api_key, preset.model());
    }

    pub fn save_llm_provider_model_config(
        &mut self,
        preset: LlmProviderPreset,
        api_key: &str,
        model: &str,
    ) {
        let mut settings = preset.settings(api_key);
        settings.model = model.trim().to_owned();
        let config = serde_json::json!({
            "base_url": settings.base_url,
            "api_key": settings.api_key,
            "model": settings.model,
            "custom_headers": settings.custom_headers,
        });
        if let Some(index) = self.llm_provider_index(preset) {
            let provider = &mut self.llm_providers[index];
            let previous_id = provider.id.clone();
            if provider.config != config {
                provider.data_consent = None;
            }
            provider.id = preset.id().to_owned();
            provider.name = preset.label().to_owned();
            provider.provider_type = "openai_compatible".to_owned();
            provider.config = config;
            if self.active.llm.as_deref() == Some(previous_id.as_str()) {
                self.active.llm = Some(preset.id().to_owned());
            }
        } else {
            self.llm_providers.push(ProviderInstance {
                id: preset.id().to_owned(),
                name: preset.label().to_owned(),
                provider_type: "openai_compatible".to_owned(),
                config,
                data_consent: None,
            });
        }
    }

    pub fn select_llm_provider(&mut self, preset: LlmProviderPreset) {
        if self.llm_provider_index(preset).is_none() {
            self.save_llm_provider_config(preset, "");
        }
        self.active.llm = self
            .llm_provider_index(preset)
            .map(|index| self.llm_providers[index].id.clone());
    }

    pub fn llm_provider_api_key(&self, preset: LlmProviderPreset) -> Option<&str> {
        self.llm_provider_index(preset).and_then(|index| {
            self.llm_providers[index]
                .config
                .get("api_key")
                .and_then(serde_json::Value::as_str)
        })
    }

    /// Returns the saved model only when the provider has a complete user configuration.
    pub fn configured_llm_provider_model(&self, preset: LlmProviderPreset) -> Option<&str> {
        let index = self.llm_provider_index(preset)?;
        let config = &self.llm_providers[index].config;
        let api_key = config.get("api_key")?.as_str()?.trim();
        let model = config.get("model")?.as_str()?.trim();
        (!api_key.is_empty() && !model.is_empty()).then_some(model)
    }

    pub fn active_llm_provider(&self) -> Option<LlmProviderPreset> {
        let active = self.active.llm.as_deref()?;
        [LlmProviderPreset::SenseNova, LlmProviderPreset::DeepSeek]
            .into_iter()
            .find(|preset| {
                self.llm_provider_index(*preset)
                    .is_some_and(|index| self.llm_providers[index].id == active)
            })
    }

    fn llm_provider_index(&self, preset: LlmProviderPreset) -> Option<usize> {
        self.llm_providers.iter().position(|provider| {
            provider.id == preset.id()
                || provider
                    .config
                    .get("base_url")
                    .and_then(serde_json::Value::as_str)
                    == Some(preset.base_url())
        })
    }
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

/// Loads and atomically saves the complete multi-instance Provider catalog.
///
/// Implementations must preserve instances with unknown provider types and
/// reject catalogs whose active identifiers do not reference the matching list.
pub trait ProviderConfigStore: Send + Sync {
    fn load_catalog(&self) -> Result<ProviderCatalog, SettingsStoreError>;
    fn save_catalog(&self, catalog: &ProviderCatalog) -> Result<(), SettingsStoreError>;
}

#[cfg(test)]
mod tests {
    use super::{LlmProviderPreset, ProviderCatalog};

    #[test]
    fn exposes_a_provider_model_only_after_configuration_is_saved() {
        let mut catalog = ProviderCatalog::default();

        assert_eq!(
            None,
            catalog.configured_llm_provider_model(LlmProviderPreset::DeepSeek)
        );

        catalog.select_llm_provider(LlmProviderPreset::DeepSeek);
        assert_eq!(
            None,
            catalog.configured_llm_provider_model(LlmProviderPreset::DeepSeek)
        );

        catalog.save_llm_provider_config(LlmProviderPreset::DeepSeek, "saved-key");
        assert_eq!(
            Some("deepseek-v4-flash"),
            catalog.configured_llm_provider_model(LlmProviderPreset::DeepSeek)
        );
    }

    #[test]
    fn saves_the_model_selected_by_the_user() {
        let mut catalog = ProviderCatalog::default();

        catalog.save_llm_provider_model_config(
            LlmProviderPreset::DeepSeek,
            "saved-key",
            "deepseek-v4-pro",
        );

        assert_eq!(
            Some("deepseek-v4-pro"),
            catalog.configured_llm_provider_model(LlmProviderPreset::DeepSeek)
        );
    }
}
