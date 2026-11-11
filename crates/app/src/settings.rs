use std::collections::BTreeMap;

use thiserror::Error;

mod llm_provider;
mod local_asr;
mod model_catalog;
pub use llm_provider::{ChatCompletionsProfile, LlmProviderPreset, LlmProviderProfile};
pub use model_catalog::LlmModelCatalog;

const MACOS_SPEECH_PROVIDER_ID: &str = "macos-speech";
const MACOS_SPEECH_PROVIDER_TYPE: &str = "macos_speech";
/// Retired streaming Paraformer identity, retained for migration and history only.
pub const PARAFORMER_PROVIDER_ID: &str = "local-paraformer";
/// Retired streaming Paraformer type, retained for migration and history only.
pub const PARAFORMER_PROVIDER_TYPE: &str = "local_paraformer";
pub const WHISPER_PROVIDER_ID: &str = "local-whisper-large-v3-turbo";
pub const WHISPER_PROVIDER_TYPE: &str = "local_whisper";
pub const QWEN3_ASR_PROVIDER_ID: &str = "local-qwen3-asr-1.7b";
pub const QWEN3_ASR_PROVIDER_TYPE: &str = "local_qwen3_asr";
pub const SENSE_VOICE_PROVIDER_ID: &str = "local-sense-voice-small-int8";
pub const SENSE_VOICE_PROVIDER_TYPE: &str = "local_sense_voice";
pub const SEACO_PROVIDER_ID: &str = "local-seaco-paraformer";
pub const SEACO_PROVIDER_TYPE: &str = "seaco-paraformer";
pub const ZIPFORMER_PROVIDER_ID: &str = "local-zipformer";
pub const ZIPFORMER_PROVIDER_TYPE: &str = "zipformer";
pub const MOONSHINE_PROVIDER_ID: &str = "local-moonshine";
pub const MOONSHINE_PROVIDER_TYPE: &str = "moonshine";
const OPENAI_TRANSCRIPTIONS_PROVIDER_TYPE: &str = "openai_transcriptions";
const VOLCENGINE_PROVIDER_TYPE: &str = "volcengine";
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
    pub profile: ChatCompletionsProfile,
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
    pub fn configure_volcengine_asr_provider(&mut self, settings: &VolcengineAsrSettings) {
        self.configure_asr_provider(
            VOLCENGINE_PROVIDER_TYPE,
            "Volcengine",
            serde_json::json!({
                "auth_mode": "api_key",
                "api_key": settings.api_key,
                "model": settings.model,
            }),
        );
    }

    pub fn configure_openai_transcriptions_asr_provider(
        &mut self,
        settings: &OpenAiCompatibleAsrSettings,
    ) {
        self.configure_asr_provider(
            OPENAI_TRANSCRIPTIONS_PROVIDER_TYPE,
            "自定义兼容接口",
            serde_json::json!({
                "base_url": settings.base_url,
                "api_key": settings.api_key,
                "model": settings.model,
            }),
        );
    }

    pub fn select_macos_speech_provider(&mut self) {
        let provider_id = self
            .asr_providers
            .iter()
            .find(|provider| provider.provider_type == MACOS_SPEECH_PROVIDER_TYPE)
            .map(|provider| provider.id.clone())
            .unwrap_or_else(|| {
                self.asr_providers.push(ProviderInstance {
                    id: MACOS_SPEECH_PROVIDER_ID.to_owned(),
                    name: "macOS Dictation".to_owned(),
                    provider_type: MACOS_SPEECH_PROVIDER_TYPE.to_owned(),
                    config: serde_json::json!({}),
                    data_consent: None,
                });
                MACOS_SPEECH_PROVIDER_ID.to_owned()
            });
        self.active.asr = Some(provider_id);
    }

    pub fn macos_speech_is_active(&self) -> bool {
        self.active.asr.as_deref().is_some_and(|active| {
            self.asr_providers.iter().any(|provider| {
                provider.id == active && provider.provider_type == MACOS_SPEECH_PROVIDER_TYPE
            })
        })
    }

    pub fn select_volcengine_asr_provider(&mut self) -> bool {
        self.select_existing_asr_provider(VOLCENGINE_PROVIDER_TYPE)
    }

    pub fn select_openai_transcriptions_asr_provider(&mut self) -> bool {
        self.select_existing_asr_provider(OPENAI_TRANSCRIPTIONS_PROVIDER_TYPE)
    }

    fn select_existing_asr_provider(&mut self, provider_type: &str) -> bool {
        let Some(provider_id) = self
            .asr_providers
            .iter()
            .find(|provider| provider.provider_type == provider_type)
            .map(|provider| provider.id.clone())
        else {
            return false;
        };
        self.active.asr = Some(provider_id);
        true
    }

    fn configure_asr_provider(
        &mut self,
        provider_type: &str,
        name: &str,
        config: serde_json::Value,
    ) {
        if let Some(provider) = self
            .asr_providers
            .iter_mut()
            .find(|provider| provider.provider_type == provider_type)
        {
            provider.config = config;
            return;
        }
        self.asr_providers.push(ProviderInstance {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_owned(),
            provider_type: provider_type.to_owned(),
            config,
            data_consent: None,
        });
    }

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

    pub fn save_llm_provider_endpoint_config(
        &mut self,
        preset: LlmProviderPreset,
        base_url: &str,
        api_key: &str,
        model: &str,
    ) {
        let mut settings = preset.settings(api_key);
        settings.base_url = base_url.trim().trim_end_matches('/').to_owned();
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
                profile: ChatCompletionsProfile::Portable,
            },
        );
    }

    fn save_llm_provider_settings(
        &mut self,
        preset: LlmProviderPreset,
        settings: ChatCompletionsLlmSettings,
    ) {
        let model_list_url = preset.model_list_url_for(&settings.base_url);
        let profile = settings.profile;
        let selected_model = settings.model.clone();
        let mut config = provider_config(&settings);
        if let Some(index) = self.llm_provider_index(preset) {
            let provider = &mut self.llm_providers[index];
            let previous_id = provider.id.clone();
            if provider.config.get("base_url") != config.get("base_url") {
                provider.data_consent = None;
            }
            model_catalog::preserve_model_catalogs(&provider.config, &mut config);
            model_catalog::update_cached_selection(
                &mut config,
                &model_list_url,
                profile,
                &selected_model,
            );
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
                LlmProviderPreset::Custom => {
                    self.save_custom_llm_provider_config("", "", "");
                }
                preset => self.save_llm_provider_config(preset, ""),
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
            profile: LlmProviderPreset::from_id_or_base_url(
                &provider.id,
                provider.config.get("base_url")?.as_str()?,
            )
            .map(|known| known.profile().chat_completions)
            .or_else(|| {
                provider
                    .config
                    .get("profile")
                    .and_then(serde_json::Value::as_str)
                    .and_then(ChatCompletionsProfile::from_id)
            })
            .unwrap_or_default(),
        })
    }

    pub fn active_llm_provider(&self) -> Option<LlmProviderPreset> {
        let active = self.active.llm.as_deref()?;
        LlmProviderPreset::BUILT_INS
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
                && !LlmProviderPreset::BUILT_INS.iter().any(|builtin| {
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
        "profile": settings.profile.id(),
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
/// The retired streaming Paraformer identity and type must never remain active:
/// normalize legacy selections to no ASR during load and reject new selections.
pub trait ProviderConfigStore: Send + Sync {
    fn load_catalog(&self) -> Result<ProviderCatalog, SettingsStoreError>;
    fn save_catalog(&self, catalog: &ProviderCatalog) -> Result<(), SettingsStoreError>;
}

#[cfg(test)]
mod tests;
