use std::collections::BTreeMap;

use thiserror::Error;

mod llm_provider;
mod local_asr;
mod model_catalog;
pub use llm_provider::{ChatCompletionsProfile, LlmProviderPreset, LlmProviderProfile};
pub use model_catalog::LlmModelCatalog;

const MACOS_SPEECH_PROVIDER_ID: &str = "macos-speech";
const MACOS_SPEECH_PROVIDER_TYPE: &str = "macos_speech";
pub const PARAFORMER_PROVIDER_ID: &str = "local-paraformer";
pub const PARAFORMER_PROVIDER_TYPE: &str = "local_paraformer";
pub const WHISPER_PROVIDER_ID: &str = "local-whisper-large-v3-turbo";
pub const WHISPER_PROVIDER_TYPE: &str = "local_whisper";
pub const QWEN3_ASR_PROVIDER_ID: &str = "local-qwen3-asr-1.7b";
pub const QWEN3_ASR_PROVIDER_TYPE: &str = "local_qwen3_asr";
pub const SENSE_VOICE_PROVIDER_ID: &str = "local-sense-voice-small-int8";
pub const SENSE_VOICE_PROVIDER_TYPE: &str = "local_sense_voice";
const OPENAI_TRANSCRIPTIONS_PROVIDER_TYPE: &str = "openai_transcriptions";
const VOLCENGINE_PROVIDER_TYPE: &str = "volcengine";
const PARAFORMER_PUNCTUATION_MODE_KEY: &str = "punctuation_mode";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ParaformerPunctuationMode {
    #[default]
    Llm,
    Local,
}

impl ParaformerPunctuationMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Llm => "llm",
            Self::Local => "local",
        }
    }
}

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

    pub fn select_paraformer_provider(&mut self) {
        self.ensure_paraformer_provider();
        self.active.asr = Some(PARAFORMER_PROVIDER_ID.to_owned());
    }

    pub fn paraformer_punctuation_mode(&self) -> ParaformerPunctuationMode {
        self.asr_providers
            .iter()
            .find(|provider| {
                provider.id == PARAFORMER_PROVIDER_ID
                    && provider.provider_type == PARAFORMER_PROVIDER_TYPE
            })
            .and_then(|provider| provider.config.get(PARAFORMER_PUNCTUATION_MODE_KEY))
            .and_then(serde_json::Value::as_str)
            .filter(|mode| *mode == ParaformerPunctuationMode::Local.as_str())
            .map(|_| ParaformerPunctuationMode::Local)
            .unwrap_or_default()
    }

    pub fn set_paraformer_punctuation_mode(&mut self, mode: ParaformerPunctuationMode) {
        let provider = self.ensure_paraformer_provider();
        if !provider.config.is_object() {
            provider.config = serde_json::json!({});
        }
        if let Some(config) = provider.config.as_object_mut() {
            config.insert(
                PARAFORMER_PUNCTUATION_MODE_KEY.to_owned(),
                serde_json::Value::String(mode.as_str().to_owned()),
            );
        }
    }

    pub fn paraformer_is_active(&self) -> bool {
        self.active.asr.as_deref() == Some(PARAFORMER_PROVIDER_ID)
            && self.asr_providers.iter().any(|provider| {
                provider.id == PARAFORMER_PROVIDER_ID
                    && provider.provider_type == PARAFORMER_PROVIDER_TYPE
            })
    }

    pub fn clear_paraformer_selection(&mut self) -> bool {
        if self.active.asr.as_deref() != Some(PARAFORMER_PROVIDER_ID) {
            return false;
        }
        self.active.asr = None;
        true
    }

    pub fn select_whisper_provider(&mut self) {
        if !self
            .asr_providers
            .iter()
            .any(|provider| provider.id == WHISPER_PROVIDER_ID)
        {
            self.asr_providers.push(ProviderInstance {
                id: WHISPER_PROVIDER_ID.to_owned(),
                name: "Whisper large-v3-turbo".to_owned(),
                provider_type: WHISPER_PROVIDER_TYPE.to_owned(),
                config: serde_json::json!({}),
                data_consent: None,
            });
        }
        self.active.asr = Some(WHISPER_PROVIDER_ID.to_owned());
    }

    pub fn whisper_is_active(&self) -> bool {
        self.active.asr.as_deref() == Some(WHISPER_PROVIDER_ID)
            && self.asr_providers.iter().any(|provider| {
                provider.id == WHISPER_PROVIDER_ID
                    && provider.provider_type == WHISPER_PROVIDER_TYPE
            })
    }

    pub fn clear_whisper_selection(&mut self) -> bool {
        if self.active.asr.as_deref() != Some(WHISPER_PROVIDER_ID) {
            return false;
        }
        self.active.asr = None;
        true
    }

    pub fn select_qwen3_asr_provider(&mut self) {
        if !self
            .asr_providers
            .iter()
            .any(|provider| provider.id == QWEN3_ASR_PROVIDER_ID)
        {
            self.asr_providers.push(ProviderInstance {
                id: QWEN3_ASR_PROVIDER_ID.to_owned(),
                name: "Qwen3-ASR 1.7B INT8".to_owned(),
                provider_type: QWEN3_ASR_PROVIDER_TYPE.to_owned(),
                config: serde_json::json!({}),
                data_consent: None,
            });
        }
        self.active.asr = Some(QWEN3_ASR_PROVIDER_ID.to_owned());
    }

    pub fn qwen3_asr_is_active(&self) -> bool {
        self.active.asr.as_deref() == Some(QWEN3_ASR_PROVIDER_ID)
            && self.asr_providers.iter().any(|provider| {
                provider.id == QWEN3_ASR_PROVIDER_ID
                    && provider.provider_type == QWEN3_ASR_PROVIDER_TYPE
            })
    }

    pub fn clear_qwen3_asr_selection(&mut self) -> bool {
        if self.active.asr.as_deref() != Some(QWEN3_ASR_PROVIDER_ID) {
            return false;
        }
        self.active.asr = None;
        true
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

    fn ensure_paraformer_provider(&mut self) -> &mut ProviderInstance {
        let index = self
            .asr_providers
            .iter()
            .position(|provider| provider.id == PARAFORMER_PROVIDER_ID)
            .unwrap_or_else(|| {
                self.asr_providers.push(ProviderInstance {
                    id: PARAFORMER_PROVIDER_ID.to_owned(),
                    name: "Paraformer".to_owned(),
                    provider_type: PARAFORMER_PROVIDER_TYPE.to_owned(),
                    config: serde_json::json!({}),
                    data_consent: None,
                });
                self.asr_providers.len() - 1
            });
        &mut self.asr_providers[index]
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
        let model_list_url = if preset == LlmProviderPreset::Custom {
            format!("{}/models", settings.base_url.trim().trim_end_matches('/'))
        } else {
            preset.model_list_url().to_owned()
        };
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
pub trait ProviderConfigStore: Send + Sync {
    fn load_catalog(&self) -> Result<ProviderCatalog, SettingsStoreError>;
    fn save_catalog(&self, catalog: &ProviderCatalog) -> Result<(), SettingsStoreError>;
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        ActiveProviders, ChatCompletionsLlmSettings, ChatCompletionsProfile, LlmProviderPreset,
        MACOS_SPEECH_PROVIDER_ID, MACOS_SPEECH_PROVIDER_TYPE, OPENAI_TRANSCRIPTIONS_PROVIDER_TYPE,
        OpenAiCompatibleAsrSettings, PARAFORMER_PROVIDER_ID, PARAFORMER_PROVIDER_TYPE,
        ParaformerPunctuationMode, ProviderCatalog, ProviderDataConsent, ProviderInstance,
        QWEN3_ASR_PROVIDER_ID, QWEN3_ASR_PROVIDER_TYPE, SENSE_VOICE_PROVIDER_ID,
        SENSE_VOICE_PROVIDER_TYPE, VOLCENGINE_PROVIDER_TYPE, VolcengineAsrSettings,
        WHISPER_PROVIDER_ID, WHISPER_PROVIDER_TYPE, provider_config,
    };

    #[test]
    fn built_in_provider_configurations_remain_independent_when_switching() {
        let providers = [
            LlmProviderPreset::Qwen,
            LlmProviderPreset::VolcengineArk,
            LlmProviderPreset::OpenAi,
            LlmProviderPreset::Kimi,
            LlmProviderPreset::Gemini,
            LlmProviderPreset::OpenRouter,
            LlmProviderPreset::ZhipuGlm,
            LlmProviderPreset::MiniMax,
        ];
        let mut catalog = ProviderCatalog::default();

        for (index, preset) in providers.into_iter().enumerate() {
            let api_key = format!("key-{index}");
            let model = format!("model-{index}");
            catalog.save_llm_provider_model_config(preset, &api_key, &model);
            catalog.select_llm_provider(preset);

            assert_eq!(Some(preset), catalog.active_llm_provider());
            assert_eq!(Some(api_key.as_str()), catalog.llm_provider_api_key(preset));
            assert_eq!(
                Some(model.as_str()),
                catalog.configured_llm_provider_model(preset)
            );
            let Some(settings) = catalog.llm_provider_settings(preset) else {
                panic!("saved provider settings should remain available");
            };
            assert_eq!(preset.base_url(), settings.base_url);
            assert_eq!(preset.profile().chat_completions, settings.profile);
        }

        assert_eq!(providers.len(), catalog.llm_providers.len());
    }

    #[test]
    fn qwen_preserves_a_workspace_specific_endpoint() {
        let mut catalog = ProviderCatalog::default();
        let endpoint = "https://workspace.cn-beijing.maas.aliyuncs.com/compatible-mode/v1";

        catalog.save_llm_provider_endpoint_config(
            LlmProviderPreset::Qwen,
            endpoint,
            "qwen-key",
            "qwen-plus",
        );

        let Some(settings) = catalog.llm_provider_settings(LlmProviderPreset::Qwen) else {
            panic!("saved Qwen settings should remain available");
        };
        assert_eq!(endpoint, settings.base_url);
        assert_eq!(ChatCompletionsProfile::Qwen, settings.profile);
    }

    #[test]
    fn static_catalog_providers_expose_recommended_models_without_a_models_endpoint() {
        assert_eq!(
            ["qwen-plus", "qwen-flash"],
            LlmProviderPreset::Qwen.recommended_models()
        );
        assert_eq!(
            ["doubao-seed-2-0-lite-260215"],
            LlmProviderPreset::VolcengineArk.recommended_models()
        );
        assert_eq!(
            ["glm-4.7-flash", "glm-5.2"],
            LlmProviderPreset::ZhipuGlm.recommended_models()
        );
        for preset in [
            LlmProviderPreset::Qwen,
            LlmProviderPreset::VolcengineArk,
            LlmProviderPreset::ZhipuGlm,
        ] {
            assert!(!preset.supports_remote_model_discovery());
        }
    }

    #[test]
    fn minimax_uses_the_official_openai_compatible_model_catalog() {
        let preset = LlmProviderPreset::MiniMax;

        assert_eq!("https://api.minimaxi.com/v1", preset.base_url());
        assert_eq!("MiniMax-M3", preset.model());
        assert_eq!(["MiniMax-M3", "MiniMax-M2.7"], preset.recommended_models());
        assert_eq!(
            "https://api.minimaxi.com/v1/models",
            preset.model_list_url()
        );
        assert!(preset.supports_remote_model_discovery());
        assert_eq!(
            ChatCompletionsProfile::MiniMax,
            preset.profile().chat_completions
        );
    }

    #[test]
    fn legacy_deepseek_config_recovers_protocol_from_provider_identity() {
        let mut catalog = ProviderCatalog::default();
        catalog.llm_providers.push(ProviderInstance {
            id: "deepseek".to_owned(),
            name: "DeepSeek".to_owned(),
            provider_type: "openai_compatible".to_owned(),
            config: serde_json::json!({
                "base_url": "https://api.deepseek.com",
                "api_key": "saved",
                "model": "renamed-model",
                "custom_headers": {}
            }),
            data_consent: None,
        });

        let settings = catalog.llm_provider_settings(LlmProviderPreset::DeepSeek);

        assert_eq!(
            Some(ChatCompletionsProfile::DeepSeek),
            settings.map(|settings| settings.profile)
        );
    }

    #[test]
    fn selecting_paraformer_creates_one_stable_provider() {
        let mut catalog = ProviderCatalog::default();

        catalog.select_paraformer_provider();
        catalog.select_paraformer_provider();

        assert!(catalog.paraformer_is_active());
        assert_eq!(Some(PARAFORMER_PROVIDER_ID), catalog.active.asr.as_deref());
        assert_eq!(
            1,
            catalog
                .asr_providers
                .iter()
                .filter(|provider| provider.provider_type == PARAFORMER_PROVIDER_TYPE)
                .count()
        );
    }

    #[test]
    fn paraformer_punctuation_mode_defaults_to_llm_and_round_trips_local() {
        let mut catalog = ProviderCatalog::default();

        assert_eq!(
            ParaformerPunctuationMode::Llm,
            catalog.paraformer_punctuation_mode()
        );
        catalog.set_paraformer_punctuation_mode(ParaformerPunctuationMode::Local);

        assert_eq!(
            ParaformerPunctuationMode::Local,
            catalog.paraformer_punctuation_mode()
        );
        assert_eq!(None, catalog.active.asr);
        assert_eq!(1, catalog.asr_providers.len());
    }

    #[test]
    fn selecting_whisper_creates_one_stable_provider() {
        let mut catalog = ProviderCatalog::default();

        catalog.select_whisper_provider();
        catalog.select_whisper_provider();

        assert!(catalog.whisper_is_active());
        assert_eq!(Some(WHISPER_PROVIDER_ID), catalog.active.asr.as_deref());
        assert_eq!(
            1,
            catalog
                .asr_providers
                .iter()
                .filter(|provider| provider.provider_type == WHISPER_PROVIDER_TYPE)
                .count()
        );
    }

    #[test]
    fn selecting_qwen3_asr_creates_one_stable_provider() {
        let mut catalog = ProviderCatalog::default();
        catalog.select_qwen3_asr_provider();
        catalog.select_qwen3_asr_provider();
        assert!(catalog.qwen3_asr_is_active());
        assert_eq!(Some(QWEN3_ASR_PROVIDER_ID), catalog.active.asr.as_deref());
        assert_eq!(
            1,
            catalog
                .asr_providers
                .iter()
                .filter(|provider| provider.provider_type == QWEN3_ASR_PROVIDER_TYPE)
                .count()
        );
    }

    #[test]
    fn selecting_sense_voice_creates_one_stable_provider() {
        let mut catalog = ProviderCatalog::default();
        catalog.select_sense_voice_provider();
        catalog.select_sense_voice_provider();

        assert!(catalog.sense_voice_is_active());
        assert_eq!(Some(SENSE_VOICE_PROVIDER_ID), catalog.active.asr.as_deref());
        assert_eq!(
            1,
            catalog
                .asr_providers
                .iter()
                .filter(|provider| provider.provider_type == SENSE_VOICE_PROVIDER_TYPE)
                .count()
        );
        assert!(catalog.clear_sense_voice_selection());
        assert!(!catalog.sense_voice_is_active());
    }

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
                    profile: ChatCompletionsProfile::Portable,
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

    #[test]
    fn selecting_macos_speech_preserves_other_asr_providers() {
        let mut catalog = ProviderCatalog {
            active: ActiveProviders {
                asr: Some("volcengine-one".to_owned()),
                llm: None,
            },
            asr_providers: vec![ProviderInstance {
                id: "volcengine-one".to_owned(),
                name: "Volcengine".to_owned(),
                provider_type: "volcengine".to_owned(),
                config: serde_json::json!({"api_key": "saved", "model": "model"}),
                data_consent: None,
            }],
            llm_providers: Vec::new(),
        };

        catalog.select_macos_speech_provider();

        assert!(catalog.macos_speech_is_active());
        assert_eq!(2, catalog.asr_providers.len());
        assert_eq!(
            Some("saved"),
            catalog.asr_providers[0]
                .config
                .get("api_key")
                .and_then(serde_json::Value::as_str)
        );
    }

    #[test]
    fn selecting_macos_speech_reuses_its_stable_provider() {
        let mut catalog = ProviderCatalog::default();

        catalog.select_macos_speech_provider();
        catalog.select_macos_speech_provider();

        assert!(catalog.macos_speech_is_active());
        assert_eq!(1, catalog.asr_providers.len());
    }

    #[test]
    fn clearing_paraformer_preserves_its_provider_configuration() {
        let mut catalog = ProviderCatalog::default();
        catalog.select_paraformer_provider();

        assert!(catalog.clear_paraformer_selection());
        assert!(!catalog.paraformer_is_active());
        assert_eq!(1, catalog.asr_providers.len());
        assert!(!catalog.clear_paraformer_selection());
    }

    #[test]
    fn selecting_a_configured_cloud_asr_provider_updates_only_the_active_provider() {
        let mut catalog = ProviderCatalog {
            active: ActiveProviders {
                asr: Some(MACOS_SPEECH_PROVIDER_ID.to_owned()),
                llm: None,
            },
            asr_providers: vec![
                ProviderInstance {
                    id: MACOS_SPEECH_PROVIDER_ID.to_owned(),
                    name: "macOS Dictation".to_owned(),
                    provider_type: MACOS_SPEECH_PROVIDER_TYPE.to_owned(),
                    config: serde_json::json!({}),
                    data_consent: None,
                },
                ProviderInstance {
                    id: "volcengine-one".to_owned(),
                    name: "Volcengine".to_owned(),
                    provider_type: VOLCENGINE_PROVIDER_TYPE.to_owned(),
                    config: serde_json::json!({"api_key": "saved", "model": "model"}),
                    data_consent: None,
                },
                ProviderInstance {
                    id: "custom-one".to_owned(),
                    name: "Custom".to_owned(),
                    provider_type: OPENAI_TRANSCRIPTIONS_PROVIDER_TYPE.to_owned(),
                    config: serde_json::json!({"api_key": "saved", "model": "model"}),
                    data_consent: None,
                },
            ],
            llm_providers: Vec::new(),
        };

        assert!(catalog.select_volcengine_asr_provider());
        assert_eq!(Some("volcengine-one"), catalog.active.asr.as_deref());
        assert_eq!(3, catalog.asr_providers.len());

        assert!(catalog.select_openai_transcriptions_asr_provider());
        assert_eq!(Some("custom-one"), catalog.active.asr.as_deref());
        assert_eq!(3, catalog.asr_providers.len());
    }

    #[test]
    fn selecting_an_unconfigured_cloud_asr_provider_keeps_the_current_provider() {
        let mut catalog = ProviderCatalog {
            active: ActiveProviders {
                asr: Some(MACOS_SPEECH_PROVIDER_ID.to_owned()),
                llm: None,
            },
            asr_providers: vec![ProviderInstance {
                id: MACOS_SPEECH_PROVIDER_ID.to_owned(),
                name: "macOS Dictation".to_owned(),
                provider_type: MACOS_SPEECH_PROVIDER_TYPE.to_owned(),
                config: serde_json::json!({}),
                data_consent: None,
            }],
            llm_providers: Vec::new(),
        };

        assert!(!catalog.select_volcengine_asr_provider());
        assert_eq!(
            Some(MACOS_SPEECH_PROVIDER_ID),
            catalog.active.asr.as_deref()
        );
    }

    #[test]
    fn configuring_cloud_asr_providers_preserves_the_current_provider() {
        let mut catalog = ProviderCatalog::default();
        catalog.select_macos_speech_provider();

        catalog.configure_volcengine_asr_provider(&VolcengineAsrSettings {
            enabled: true,
            api_key: "volc-key".to_owned(),
            model: "volc-model".to_owned(),
        });
        catalog.configure_openai_transcriptions_asr_provider(&OpenAiCompatibleAsrSettings {
            enabled: true,
            base_url: "https://example.com/v1".to_owned(),
            api_key: "custom-key".to_owned(),
            model: "custom-model".to_owned(),
        });

        assert!(catalog.macos_speech_is_active());
        assert_eq!(3, catalog.asr_providers.len());
        assert_eq!(
            Some("volc-key"),
            catalog
                .asr_providers
                .iter()
                .find(|provider| provider.provider_type == VOLCENGINE_PROVIDER_TYPE)
                .and_then(|provider| provider.config.get("api_key"))
                .and_then(serde_json::Value::as_str)
        );
        assert_eq!(
            Some("custom-model"),
            catalog
                .asr_providers
                .iter()
                .find(|provider| provider.provider_type == OPENAI_TRANSCRIPTIONS_PROVIDER_TYPE)
                .and_then(|provider| provider.config.get("model"))
                .and_then(serde_json::Value::as_str)
        );
    }
}
