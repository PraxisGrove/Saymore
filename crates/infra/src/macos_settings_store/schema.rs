use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use template_app::{
    ActiveProviders, AsrSettings, ChatCompletionsLlmSettings, LlmSettings,
    OpenAiCompatibleAsrSettings, ProviderCatalog, ProviderDataConsent, ProviderInstance,
    SaymoreSettings, SettingsStoreError, VolcengineAsrSettings,
};
use uuid::Uuid;

use super::{
    CHAT_COMPLETIONS_TYPE, CONFIG_VERSION, OPENAI_TRANSCRIPTIONS_TYPE, VOLCENGINE_TYPE,
    active_provider, endpoint_fingerprint, json_error, validate_catalog,
};

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct LegacySettings {
    #[serde(rename = "version")]
    _version: u32,
    #[serde(default)]
    asr: StoredAsrSettings,
    #[serde(default)]
    llm: StoredLlmSettings,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct StoredAsrSettings {
    #[serde(default)]
    volcengine: StoredVolcengineAsrSettings,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct StoredVolcengineAsrSettings {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    auth_mode: StoredAuthMode,
    #[serde(default)]
    api_key: String,
    #[serde(default)]
    model: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct StoredOpenAiCompatibleAsrSettings {
    #[serde(default)]
    base_url: String,
    #[serde(default)]
    api_key: String,
    #[serde(default)]
    model: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StoredAuthMode {
    #[default]
    ApiKey,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct StoredLlmSettings {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    confirmed_base_url: String,
    #[serde(default)]
    chat_completions: StoredChatCompletionsLlmSettings,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct StoredChatCompletionsLlmSettings {
    #[serde(default)]
    base_url: String,
    #[serde(default)]
    api_key: String,
    #[serde(default)]
    model: String,
    #[serde(default)]
    custom_headers: BTreeMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct StoredCatalog {
    version: u32,
    #[serde(default)]
    active: StoredActiveProviders,
    #[serde(default)]
    asr_providers: Vec<StoredProviderInstance>,
    #[serde(default)]
    llm_providers: Vec<StoredProviderInstance>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct StoredActiveProviders {
    #[serde(default)]
    asr: Option<String>,
    #[serde(default)]
    llm: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredProviderInstance {
    id: String,
    name: String,
    #[serde(rename = "type")]
    provider_type: String,
    config: serde_json::Value,
    #[serde(default)]
    data_consent: Option<StoredDataConsent>,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredDataConsent {
    fingerprint: String,
}

impl From<&ProviderCatalog> for StoredCatalog {
    fn from(catalog: &ProviderCatalog) -> Self {
        Self {
            version: CONFIG_VERSION,
            active: StoredActiveProviders {
                asr: catalog.active.asr.clone(),
                llm: catalog.active.llm.clone(),
            },
            asr_providers: catalog
                .asr_providers
                .iter()
                .map(StoredProviderInstance::from)
                .collect(),
            llm_providers: catalog
                .llm_providers
                .iter()
                .map(StoredProviderInstance::from)
                .collect(),
        }
    }
}

impl From<&ProviderInstance> for StoredProviderInstance {
    fn from(provider: &ProviderInstance) -> Self {
        Self {
            id: provider.id.clone(),
            name: provider.name.clone(),
            provider_type: provider.provider_type.clone(),
            config: provider.config.clone(),
            data_consent: provider
                .data_consent
                .as_ref()
                .map(|consent| StoredDataConsent {
                    fingerprint: consent.fingerprint.clone(),
                }),
        }
    }
}

impl StoredCatalog {
    pub(super) fn into_catalog(self) -> ProviderCatalog {
        ProviderCatalog {
            active: ActiveProviders {
                asr: self.active.asr,
                llm: self.active.llm,
            },
            asr_providers: self
                .asr_providers
                .into_iter()
                .map(StoredProviderInstance::into_provider)
                .collect(),
            llm_providers: self
                .llm_providers
                .into_iter()
                .map(StoredProviderInstance::into_provider)
                .collect(),
        }
    }
}

impl StoredProviderInstance {
    fn into_provider(self) -> ProviderInstance {
        ProviderInstance {
            id: self.id,
            name: self.name,
            provider_type: self.provider_type,
            config: self.config,
            data_consent: self.data_consent.map(|consent| ProviderDataConsent {
                fingerprint: consent.fingerprint,
            }),
        }
    }
}

pub(super) fn legacy_catalog(legacy: LegacySettings) -> ProviderCatalog {
    let mut catalog = ProviderCatalog::default();
    let asr = legacy.asr.volcengine;
    if asr.enabled || !asr.api_key.is_empty() || !asr.model.is_empty() {
        let id = Uuid::new_v4().to_string();
        if asr.enabled {
            catalog.active.asr = Some(id.clone());
        }
        catalog.asr_providers.push(ProviderInstance {
            id,
            name: "Volcengine".to_owned(),
            provider_type: VOLCENGINE_TYPE.to_owned(),
            config: serde_json::json!({
                "auth_mode": "api_key",
                "api_key": asr.api_key,
                "model": asr.model
            }),
            data_consent: None,
        });
    }
    let llm = legacy.llm;
    let config = llm.chat_completions;
    if llm.enabled
        || !config.base_url.is_empty()
        || !config.api_key.is_empty()
        || !config.model.is_empty()
    {
        let id = Uuid::new_v4().to_string();
        if llm.enabled {
            catalog.active.llm = Some(id.clone());
        }
        let data_consent = (!llm.confirmed_base_url.is_empty()
            && llm.confirmed_base_url == config.base_url)
            .then(|| ProviderDataConsent {
                fingerprint: endpoint_fingerprint(&config.base_url),
            });
        catalog.llm_providers.push(ProviderInstance {
            id,
            name: "OpenAI-compatible".to_owned(),
            provider_type: CHAT_COMPLETIONS_TYPE.to_owned(),
            config: serde_json::json!({
                "base_url": config.base_url,
                "api_key": config.api_key,
                "model": config.model,
                "custom_headers": config.custom_headers
            }),
            data_consent,
        });
    }
    catalog
}

pub(super) fn catalog_to_settings(
    catalog: ProviderCatalog,
) -> Result<SaymoreSettings, SettingsStoreError> {
    validate_catalog(&catalog)?;
    let active_asr = catalog.active.asr.as_deref();
    let volcengine = match catalog
        .asr_providers
        .iter()
        .find(|provider| provider.provider_type == VOLCENGINE_TYPE)
    {
        Some(provider) => {
            let stored: StoredVolcengineAsrSettings =
                serde_json::from_value(provider.config.clone()).map_err(json_error)?;
            VolcengineAsrSettings {
                enabled: active_asr == Some(provider.id.as_str()),
                api_key: stored.api_key,
                model: stored.model,
            }
        }
        None => VolcengineAsrSettings::default(),
    };
    let openai_compatible = match catalog
        .asr_providers
        .iter()
        .find(|provider| provider.provider_type == OPENAI_TRANSCRIPTIONS_TYPE)
    {
        Some(provider) => {
            let stored: StoredOpenAiCompatibleAsrSettings =
                serde_json::from_value(provider.config.clone()).map_err(json_error)?;
            OpenAiCompatibleAsrSettings {
                enabled: active_asr == Some(provider.id.as_str()),
                base_url: stored.base_url,
                api_key: stored.api_key,
                model: stored.model,
            }
        }
        None => OpenAiCompatibleAsrSettings::default(),
    };
    let active_llm = catalog.active.llm.as_deref();
    let llm_provider = active_provider(&catalog.llm_providers, active_llm).or_else(|| {
        catalog
            .llm_providers
            .iter()
            .find(|provider| provider.provider_type == CHAT_COMPLETIONS_TYPE)
    });
    let (enabled, confirmed_base_url, chat_completions) =
        match llm_provider.filter(|provider| provider.provider_type == CHAT_COMPLETIONS_TYPE) {
            Some(provider) => {
                let stored: StoredChatCompletionsLlmSettings =
                    serde_json::from_value(provider.config.clone()).map_err(json_error)?;
                let confirmed = provider
                    .data_consent
                    .as_ref()
                    .filter(|consent| consent.fingerprint == endpoint_fingerprint(&stored.base_url))
                    .map(|_| stored.base_url.clone())
                    .unwrap_or_default();
                let enabled = active_llm == Some(provider.id.as_str()) && !confirmed.is_empty();
                (
                    enabled,
                    confirmed,
                    ChatCompletionsLlmSettings {
                        base_url: stored.base_url,
                        api_key: stored.api_key,
                        model: stored.model,
                        custom_headers: stored.custom_headers,
                    },
                )
            }
            None => (false, String::new(), ChatCompletionsLlmSettings::default()),
        };
    Ok(SaymoreSettings {
        asr: AsrSettings {
            volcengine,
            openai_compatible,
        },
        llm: LlmSettings {
            enabled,
            confirmed_base_url,
            chat_completions,
        },
    })
}
