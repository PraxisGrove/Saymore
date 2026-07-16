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
    pub openai_compatible: OpenAiCompatibleAsrSettings,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VolcengineAsrSettings {
    pub enabled: bool,
    pub api_key: String,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OpenAiCompatibleAsrSettings {
    pub enabled: bool,
    pub base_url: String,
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
    Custom,
}

impl LlmProviderPreset {
    pub const ALL: [Self; 3] = [Self::SenseNova, Self::DeepSeek, Self::Custom];

    pub const fn label(self) -> &'static str {
        match self {
            Self::SenseNova => "商汤 SenseNova",
            Self::DeepSeek => "DeepSeek",
            Self::Custom => "Custom compatible API",
        }
    }

    pub const fn id(self) -> &'static str {
        match self {
            Self::SenseNova => "sensenova",
            Self::DeepSeek => "deepseek",
            Self::Custom => "custom",
        }
    }

    pub const fn base_url(self) -> &'static str {
        match self {
            Self::SenseNova => "https://token.sensenova.cn/v1",
            Self::DeepSeek => "https://api.deepseek.com",
            Self::Custom => "",
        }
    }

    pub const fn model(self) -> &'static str {
        match self {
            Self::SenseNova => "sensenova-6.7-flash-lite",
            Self::DeepSeek => "deepseek-v4-flash",
            Self::Custom => "",
        }
    }

    pub const fn model_list_url(self) -> &'static str {
        match self {
            Self::SenseNova => "https://api.sensenova.cn/v1/llm/models",
            Self::DeepSeek => "https://api.deepseek.com/models",
            Self::Custom => "",
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
        self.save_llm_provider_settings(preset, settings);
    }

    pub fn save_custom_llm_provider_config(&mut self, base_url: &str, api_key: &str, model: &str) {
        self.save_llm_provider_settings(
            LlmProviderPreset::Custom,
            ChatCompletionsLlmSettings {
                base_url: base_url.trim().trim_end_matches('/').to_owned(),
                api_key: api_key.trim().to_owned(),
                model: model.trim().to_owned(),
                custom_headers: BTreeMap::new(),
            },
        );
    }

    fn save_llm_provider_settings(
        &mut self,
        preset: LlmProviderPreset,
        settings: ChatCompletionsLlmSettings,
    ) {
        let config = provider_config(&settings);
        if let Some(index) = self.llm_provider_index(preset) {
            let provider = &mut self.llm_providers[index];
            let previous_id = provider.id.clone();
            if provider.config.get("base_url") != config.get("base_url") {
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
            match preset {
                LlmProviderPreset::SenseNova | LlmProviderPreset::DeepSeek => {
                    self.save_llm_provider_config(preset, "");
                }
                LlmProviderPreset::Custom => {
                    self.save_custom_llm_provider_config("", "", "");
                }
            }
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
        let base_url = config.get("base_url")?.as_str()?.trim();
        let api_key = config.get("api_key")?.as_str()?.trim();
        let model = config.get("model")?.as_str()?.trim();
        let credentials_ready = preset == LlmProviderPreset::Custom || !api_key.is_empty();
        (!base_url.is_empty() && credentials_ready && !model.is_empty()).then_some(model)
    }

    pub fn llm_provider_settings(
        &self,
        preset: LlmProviderPreset,
    ) -> Option<ChatCompletionsLlmSettings> {
        let provider = &self.llm_providers[self.llm_provider_index(preset)?];
        let custom_headers = provider
            .config
            .get("custom_headers")
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .ok()?
            .unwrap_or_default();
        Some(ChatCompletionsLlmSettings {
            base_url: provider.config.get("base_url")?.as_str()?.to_owned(),
            api_key: provider.config.get("api_key")?.as_str()?.to_owned(),
            model: provider.config.get("model")?.as_str()?.to_owned(),
            custom_headers,
        })
    }

    pub fn active_llm_provider(&self) -> Option<LlmProviderPreset> {
        let active = self.active.llm.as_deref()?;
        [LlmProviderPreset::SenseNova, LlmProviderPreset::DeepSeek]
            .into_iter()
            .find(|preset| {
                self.llm_provider_index(*preset)
                    .is_some_and(|index| self.llm_providers[index].id == active)
            })
            .or_else(|| {
                self.llm_providers
                    .iter()
                    .any(|provider| {
                        provider.id == active && provider.provider_type == "openai_compatible"
                    })
                    .then_some(LlmProviderPreset::Custom)
            })
    }

    fn llm_provider_index(&self, preset: LlmProviderPreset) -> Option<usize> {
        let exact = self.llm_providers.iter().position(|provider| {
            provider.id == preset.id()
                || (preset != LlmProviderPreset::Custom
                    && provider
                        .config
                        .get("base_url")
                        .and_then(serde_json::Value::as_str)
                        == Some(preset.base_url()))
        });
        if exact.is_some() || preset != LlmProviderPreset::Custom {
            return exact;
        }
        let active = self.active.llm.as_deref()?;
        self.llm_providers.iter().position(|provider| {
            provider.id == active
                && provider.provider_type == "openai_compatible"
                && ![LlmProviderPreset::SenseNova, LlmProviderPreset::DeepSeek]
                    .iter()
                    .any(|builtin| {
                        provider
                            .config
                            .get("base_url")
                            .and_then(serde_json::Value::as_str)
                            == Some(builtin.base_url())
                    })
        })
    }
}

fn provider_config(settings: &ChatCompletionsLlmSettings) -> serde_json::Value {
    serde_json::json!({
        "base_url": settings.base_url,
        "api_key": settings.api_key,
        "model": settings.model,
        "custom_headers": settings.custom_headers,
    })
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
    use std::collections::BTreeMap;

    use super::{
        ActiveProviders, ChatCompletionsLlmSettings, LlmProviderPreset, ProviderCatalog,
        ProviderDataConsent, ProviderInstance, provider_config,
    };

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

    #[test]
    fn changing_credentials_or_model_preserves_endpoint_consent() {
        let mut catalog = ProviderCatalog::default();
        catalog.save_llm_provider_config(LlmProviderPreset::DeepSeek, "old-key");
        catalog.llm_providers[0].data_consent = Some(ProviderDataConsent {
            fingerprint: "endpoint:https://api.deepseek.com".to_owned(),
        });

        catalog.save_llm_provider_model_config(
            LlmProviderPreset::DeepSeek,
            "new-key",
            "deepseek-v4-pro",
        );

        assert_eq!(
            Some("endpoint:https://api.deepseek.com"),
            catalog.llm_providers[0]
                .data_consent
                .as_ref()
                .map(|consent| consent.fingerprint.as_str())
        );
    }

    #[test]
    fn custom_provider_round_trips_user_owned_connection_settings() {
        let mut catalog = ProviderCatalog::default();

        catalog.save_custom_llm_provider_config(" http://localhost:11434/v1/ ", "", "qwen3:8b");
        catalog.select_llm_provider(LlmProviderPreset::Custom);

        assert_eq!(
            Some(LlmProviderPreset::Custom),
            catalog.active_llm_provider()
        );
        assert_eq!(
            Some("qwen3:8b"),
            catalog.configured_llm_provider_model(LlmProviderPreset::Custom)
        );
        let Some(settings) = catalog.llm_provider_settings(LlmProviderPreset::Custom) else {
            panic!("custom provider settings should be available");
        };
        assert_eq!("http://localhost:11434/v1", settings.base_url);
        assert_eq!("", settings.api_key);
        assert_eq!("qwen3:8b", settings.model);
    }

    #[test]
    fn active_generic_compatible_provider_is_adopted_as_custom() {
        let mut catalog = ProviderCatalog {
            active: ActiveProviders {
                asr: None,
                llm: Some("legacy-custom".to_owned()),
            },
            asr_providers: Vec::new(),
            llm_providers: vec![ProviderInstance {
                id: "legacy-custom".to_owned(),
                name: "Local model".to_owned(),
                provider_type: "openai_compatible".to_owned(),
                config: provider_config(&ChatCompletionsLlmSettings {
                    base_url: "http://localhost:11434/v1".to_owned(),
                    api_key: String::new(),
                    model: "qwen3:8b".to_owned(),
                    custom_headers: BTreeMap::new(),
                }),
                data_consent: None,
            }],
        };

        assert_eq!(
            Some(LlmProviderPreset::Custom),
            catalog.active_llm_provider()
        );
        catalog.save_custom_llm_provider_config("http://localhost:11434/v1", "", "qwen3:8b");
        assert_eq!(Some("custom"), catalog.active.llm.as_deref());
        assert_eq!("custom", catalog.llm_providers[0].id);
    }
}
