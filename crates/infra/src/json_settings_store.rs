use std::{
    fs,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
};

use template_app::{
    AsrProviderConfiguration, AsrSettings, ChatCompletionsProfile, LlmProviderConfiguration,
    LlmProviderPreset, LlmSettings, OpenAiCompatibleAsrSettings, ProviderCatalog,
    ProviderConfigStore, ProviderConfigurationStore, ProviderDataConsent, ProviderInstance,
    SaymoreSettings, SettingsStore, SettingsStoreError, VolcengineAsrSettings,
};
use uuid::Uuid;

use crate::app_paths::{AppEnvironment, AppPaths};

mod platform_fs;
mod schema;

use platform_fs::{atomic_replace, open_private_file, restrict_directory_permissions};
use schema::{LegacySettings, StoredCatalog, catalog_to_settings, legacy_catalog};

const CONFIG_VERSION: u32 = 3;
const VOLCENGINE_TYPE: &str = "volcengine";
const OPENAI_TRANSCRIPTIONS_TYPE: &str = "openai_transcriptions";
const CHAT_COMPLETIONS_TYPE: &str = "openai_compatible";
const LLM_DATA_SCOPE: &str = "transcript+confirmed_dictionary_terms+local_correction_fragment+recent_final_history_for_vocabulary_suggestions+refinement_parameters:v3";

pub struct JsonSettingsStore {
    path: PathBuf,
    access: Mutex<()>,
}

impl JsonSettingsStore {
    pub fn for_current_user(environment: AppEnvironment) -> Result<Self, SettingsStoreError> {
        let paths = AppPaths::for_current_user(environment)
            .map_err(|error| SettingsStoreError::Unavailable(error.to_string()))?;
        Ok(Self::at_path(paths.provider_config()))
    }

    pub fn at_path(path: PathBuf) -> Self {
        Self {
            path,
            access: Mutex::new(()),
        }
    }

    pub fn ensure_exists(&self) -> Result<(), SettingsStoreError> {
        let _guard = self.lock_access()?;
        if self.path.exists() {
            self.load_catalog_unlocked().map(|_| ())
        } else {
            self.save_catalog_unlocked(&ProviderCatalog::default())
        }
    }

    pub fn enable_llm_provider_if_unchanged(
        &self,
        expected_provider_id: &str,
        expected_base_url: &str,
        expected_api_key: &str,
    ) -> Result<bool, SettingsStoreError> {
        let _guard = self.lock_access()?;
        let mut catalog = self.load_catalog_unlocked()?;
        if catalog.active.llm.as_deref() != Some(expected_provider_id) {
            return Ok(false);
        }
        let Some(provider) = catalog
            .llm_providers
            .iter_mut()
            .find(|provider| provider.id == expected_provider_id)
        else {
            return Ok(false);
        };
        let unchanged = provider
            .config
            .get("base_url")
            .and_then(serde_json::Value::as_str)
            == Some(expected_base_url)
            && provider
                .config
                .get("api_key")
                .and_then(serde_json::Value::as_str)
                == Some(expected_api_key);
        if !unchanged {
            return Ok(false);
        }
        let Some(config) = provider.config.as_object_mut() else {
            return Err(SettingsStoreError::Invalid(
                "LLM provider configuration must be an object".to_owned(),
            ));
        };
        config.insert("enabled".to_owned(), serde_json::Value::Bool(true));
        provider.data_consent = Some(ProviderDataConsent {
            fingerprint: endpoint_fingerprint(expected_base_url),
        });
        self.save_catalog_unlocked(&catalog)?;
        Ok(true)
    }

    pub fn cache_llm_model_catalog(
        &self,
        preset: LlmProviderPreset,
        model_list_url: &str,
        profile: ChatCompletionsProfile,
        models: Vec<String>,
        selected_model: &str,
        refreshed_at_ms: i64,
    ) -> Result<bool, SettingsStoreError> {
        let _guard = self.lock_access()?;
        let mut catalog = self.load_catalog_unlocked()?;
        if !catalog.cache_llm_model_catalog(
            preset,
            model_list_url,
            profile,
            models,
            selected_model,
            refreshed_at_ms,
        ) {
            return Ok(false);
        }
        self.save_catalog_unlocked(&catalog)?;
        Ok(true)
    }

    pub fn select_cached_llm_model(
        &self,
        preset: LlmProviderPreset,
        model_list_url: &str,
        profile: ChatCompletionsProfile,
        selected_model: &str,
    ) -> Result<bool, SettingsStoreError> {
        let _guard = self.lock_access()?;
        let mut catalog = self.load_catalog_unlocked()?;
        if !catalog.select_cached_llm_model(preset, model_list_url, profile, selected_model) {
            return Ok(false);
        }
        self.save_catalog_unlocked(&catalog)?;
        Ok(true)
    }

    /// Loads both provider views from one locked filesystem snapshot.
    ///
    /// Dictation completion adapters use this to keep the executable refinement plan
    /// and persisted provider metadata consistent for one session.
    pub fn load_settings_snapshot(
        &self,
    ) -> Result<(SaymoreSettings, ProviderCatalog), SettingsStoreError> {
        let _guard = self.lock_access()?;
        let catalog = self.load_catalog_unlocked()?;
        let settings = catalog_to_settings(catalog.clone())?;
        Ok((settings, catalog))
    }

    fn lock_access(&self) -> Result<MutexGuard<'_, ()>, SettingsStoreError> {
        self.access.lock().map_err(|_| {
            SettingsStoreError::Unavailable("settings access lock was poisoned".to_owned())
        })
    }

    fn load_catalog_unlocked(&self) -> Result<ProviderCatalog, SettingsStoreError> {
        if !self.path.exists() {
            return Ok(ProviderCatalog::default());
        }
        let file = File::open(&self.path).map_err(io_error)?;
        let value: serde_json::Value =
            serde_json::from_reader(BufReader::new(file)).map_err(json_error)?;
        let version = value
            .get("version")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| SettingsStoreError::Invalid("config version is missing".to_owned()))?;
        match version {
            1 | 2 => {
                let legacy: LegacySettings = serde_json::from_value(value).map_err(json_error)?;
                let catalog = legacy_catalog(legacy);
                self.save_catalog_unlocked(&catalog)?;
                Ok(catalog)
            }
            3 => {
                let stored: StoredCatalog = serde_json::from_value(value).map_err(json_error)?;
                let catalog = stored.into_catalog();
                validate_catalog(&catalog)?;
                Ok(catalog)
            }
            other => Err(SettingsStoreError::Invalid(format!(
                "unsupported config version {other}"
            ))),
        }
    }

    fn save_catalog_unlocked(&self, catalog: &ProviderCatalog) -> Result<(), SettingsStoreError> {
        validate_catalog(catalog)?;
        let parent = self.path.parent().ok_or_else(|| {
            SettingsStoreError::Unavailable("settings path has no parent".to_owned())
        })?;
        fs::create_dir_all(parent).map_err(io_error)?;
        restrict_directory_permissions(parent)?;

        let temporary = temporary_path(&self.path);
        let result = (|| {
            let file = open_private_file(&temporary)?;
            let mut writer = BufWriter::new(file);
            serde_json::to_writer_pretty(&mut writer, &StoredCatalog::from(catalog))
                .map_err(json_error)?;
            writer.write_all(b"\n").map_err(io_error)?;
            writer.flush().map_err(io_error)?;
            writer.get_ref().sync_all().map_err(io_error)?;
            drop(writer);
            atomic_replace(&temporary, &self.path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

impl SettingsStore for JsonSettingsStore {
    fn load(&self) -> Result<SaymoreSettings, SettingsStoreError> {
        let _guard = self.lock_access()?;
        self.load_catalog_unlocked().and_then(catalog_to_settings)
    }

    fn save(&self, settings: &SaymoreSettings) -> Result<(), SettingsStoreError> {
        let _guard = self.lock_access()?;
        let mut catalog = if self.path.exists() {
            self.load_catalog_unlocked()?
        } else {
            ProviderCatalog::default()
        };
        update_asr_providers(&mut catalog, &settings.asr);
        update_llm_provider(&mut catalog, &settings.llm);
        self.save_catalog_unlocked(&catalog)
    }
}

impl ProviderConfigStore for JsonSettingsStore {
    fn load_catalog(&self) -> Result<ProviderCatalog, SettingsStoreError> {
        let _guard = self.lock_access()?;
        self.load_catalog_unlocked()
    }

    fn save_catalog(&self, catalog: &ProviderCatalog) -> Result<(), SettingsStoreError> {
        let _guard = self.lock_access()?;
        self.save_catalog_unlocked(catalog)
    }
}

impl ProviderConfigurationStore for JsonSettingsStore {
    fn save_asr_configuration(
        &self,
        candidate: &AsrProviderConfiguration,
    ) -> Result<(), SettingsStoreError> {
        let _guard = self.lock_access()?;
        let mut catalog = self.load_catalog_unlocked()?;
        match candidate {
            AsrProviderConfiguration::Volcengine(settings) => {
                catalog.configure_volcengine_asr_provider(settings);
            }
            AsrProviderConfiguration::OpenAiCompatible(settings) => {
                catalog.configure_openai_transcriptions_asr_provider(settings);
            }
        }
        self.save_catalog_unlocked(&catalog)
    }

    fn save_and_enable_llm_configuration(
        &self,
        candidate: &LlmProviderConfiguration,
    ) -> Result<(), SettingsStoreError> {
        let _guard = self.lock_access()?;
        let mut catalog = self.load_catalog_unlocked()?;
        let preset = candidate.provider();
        let settings = candidate.settings();
        if preset == LlmProviderPreset::Custom {
            catalog.save_custom_llm_provider_config(
                &settings.base_url,
                &settings.api_key,
                &settings.model,
            );
        } else if preset.base_url_editable() {
            catalog.save_llm_provider_endpoint_config(
                preset,
                &settings.base_url,
                &settings.api_key,
                &settings.model,
            );
        } else {
            catalog.save_llm_provider_model_config(preset, &settings.api_key, &settings.model);
        }
        catalog.select_llm_provider(preset);
        let provider_id = catalog.active.llm.as_deref().ok_or_else(|| {
            SettingsStoreError::Invalid("LLM provider selection failed".to_owned())
        })?;
        let provider = catalog
            .llm_providers
            .iter_mut()
            .find(|provider| provider.id == provider_id)
            .ok_or_else(|| {
                SettingsStoreError::Invalid("selected LLM provider is missing".to_owned())
            })?;
        let config = provider.config.as_object_mut().ok_or_else(|| {
            SettingsStoreError::Invalid("LLM provider configuration must be an object".to_owned())
        })?;
        config.insert("enabled".to_owned(), serde_json::Value::Bool(true));
        provider.data_consent = Some(ProviderDataConsent {
            fingerprint: endpoint_fingerprint(&settings.base_url),
        });
        self.save_catalog_unlocked(&catalog)
    }
}

fn update_asr_providers(catalog: &mut ProviderCatalog, settings: &AsrSettings) {
    let preserved_non_cloud_provider = (!settings.volcengine.enabled
        && !settings.openai_compatible.enabled)
        .then(|| catalog.active.asr.clone())
        .flatten()
        .filter(|active_id| {
            catalog.asr_providers.iter().any(|provider| {
                provider.id == *active_id
                    && !matches!(
                        provider.provider_type.as_str(),
                        VOLCENGINE_TYPE | OPENAI_TRANSCRIPTIONS_TYPE
                    )
            })
        });
    update_volcengine_asr_provider(catalog, &settings.volcengine);
    update_openai_asr_provider(catalog, &settings.openai_compatible);
    if let Some(provider_id) = preserved_non_cloud_provider {
        catalog.active.asr = Some(provider_id);
    }
}

fn update_volcengine_asr_provider(catalog: &mut ProviderCatalog, settings: &VolcengineAsrSettings) {
    let index = provider_index(
        &catalog.asr_providers,
        catalog.active.asr.as_deref(),
        VOLCENGINE_TYPE,
    );
    if settings.api_key.is_empty() && settings.model.is_empty() {
        if index.is_some_and(|index| {
            catalog.active.asr.as_deref() == Some(&catalog.asr_providers[index].id)
        }) {
            catalog.active.asr = None;
        }
        return;
    }
    let index = index.unwrap_or_else(|| {
        catalog.asr_providers.push(ProviderInstance {
            id: Uuid::new_v4().to_string(),
            name: "Volcengine".to_owned(),
            provider_type: VOLCENGINE_TYPE.to_owned(),
            config: serde_json::Value::Null,
            data_consent: None,
        });
        catalog.asr_providers.len() - 1
    });
    let provider = &mut catalog.asr_providers[index];
    provider.config = serde_json::json!({
        "auth_mode": "api_key",
        "api_key": settings.api_key,
        "model": settings.model
    });
    catalog.active.asr = settings.enabled.then(|| provider.id.clone());
}

fn update_openai_asr_provider(
    catalog: &mut ProviderCatalog,
    settings: &OpenAiCompatibleAsrSettings,
) {
    let index = provider_index(
        &catalog.asr_providers,
        catalog.active.asr.as_deref(),
        OPENAI_TRANSCRIPTIONS_TYPE,
    );
    if settings.base_url.is_empty() && settings.api_key.is_empty() && settings.model.is_empty() {
        if index.is_some_and(|index| {
            catalog.active.asr.as_deref() == Some(&catalog.asr_providers[index].id)
        }) {
            catalog.active.asr = None;
        }
        return;
    }
    let index = index.unwrap_or_else(|| {
        catalog.asr_providers.push(ProviderInstance {
            id: Uuid::new_v4().to_string(),
            name: "自定义兼容接口".to_owned(),
            provider_type: OPENAI_TRANSCRIPTIONS_TYPE.to_owned(),
            config: serde_json::Value::Null,
            data_consent: None,
        });
        catalog.asr_providers.len() - 1
    });
    let provider = &mut catalog.asr_providers[index];
    provider.config = serde_json::json!({
        "base_url": settings.base_url,
        "api_key": settings.api_key,
        "model": settings.model
    });
    if settings.enabled {
        catalog.active.asr = Some(provider.id.clone());
    }
}

fn update_llm_provider(catalog: &mut ProviderCatalog, settings: &LlmSettings) {
    let index = provider_index(
        &catalog.llm_providers,
        catalog.active.llm.as_deref(),
        CHAT_COMPLETIONS_TYPE,
    );
    let config = &settings.chat_completions;
    if index.is_none()
        && config.base_url.is_empty()
        && config.api_key.is_empty()
        && config.model.is_empty()
    {
        catalog.active.llm = None;
        return;
    }
    let index = index.unwrap_or_else(|| {
        catalog.llm_providers.push(ProviderInstance {
            id: Uuid::new_v4().to_string(),
            name: "OpenAI-compatible".to_owned(),
            provider_type: CHAT_COMPLETIONS_TYPE.to_owned(),
            config: serde_json::Value::Null,
            data_consent: None,
        });
        catalog.llm_providers.len() - 1
    });
    let provider = &mut catalog.llm_providers[index];
    let model_catalogs = provider.config.get("model_catalogs").cloned();
    let mut provider_config = serde_json::json!({
        "enabled": settings.enabled,
        "base_url": config.base_url,
        "api_key": config.api_key,
        "model": config.model,
        "custom_headers": config.custom_headers
    });
    if let (Some(model_catalogs), Some(provider_config)) =
        (model_catalogs, provider_config.as_object_mut())
    {
        provider_config.insert("model_catalogs".to_owned(), model_catalogs);
    }
    provider.config = provider_config;
    provider.data_consent = (!settings.confirmed_base_url.is_empty()
        && settings.confirmed_base_url == config.base_url)
        .then(|| ProviderDataConsent {
            fingerprint: endpoint_fingerprint(&config.base_url),
        });
    catalog.active.llm = Some(provider.id.clone());
}

fn active_provider<'a>(
    providers: &'a [ProviderInstance],
    active: Option<&str>,
) -> Option<&'a ProviderInstance> {
    active.and_then(|id| providers.iter().find(|provider| provider.id == id))
}

fn provider_index(
    providers: &[ProviderInstance],
    active: Option<&str>,
    provider_type: &str,
) -> Option<usize> {
    active
        .and_then(|id| providers.iter().position(|provider| provider.id == id))
        .filter(|index| providers[*index].provider_type == provider_type)
        .or_else(|| {
            providers
                .iter()
                .position(|provider| provider.provider_type == provider_type)
        })
}

fn endpoint_fingerprint(base_url: &str) -> String {
    format!(
        "provider:{CHAT_COMPLETIONS_TYPE}|endpoint:{}|scope:{LLM_DATA_SCOPE}",
        base_url.trim()
    )
}

fn validate_catalog(catalog: &ProviderCatalog) -> Result<(), SettingsStoreError> {
    let mut ids = std::collections::BTreeSet::new();
    for provider in catalog
        .asr_providers
        .iter()
        .chain(catalog.llm_providers.iter())
    {
        if provider.id.trim().is_empty()
            || provider.name.trim().is_empty()
            || provider.provider_type.trim().is_empty()
            || !ids.insert(provider.id.as_str())
        {
            return Err(SettingsStoreError::Invalid(
                "provider catalog contains an empty or duplicate identity".to_owned(),
            ));
        }
    }
    if catalog.active.asr.as_ref().is_some_and(|active| {
        !catalog
            .asr_providers
            .iter()
            .any(|provider| &provider.id == active)
    }) || catalog.active.llm.as_ref().is_some_and(|active| {
        !catalog
            .llm_providers
            .iter()
            .any(|provider| &provider.id == active)
    }) {
        return Err(SettingsStoreError::Invalid(
            "active provider does not reference its matching provider list".to_owned(),
        ));
    }
    Ok(())
}

fn temporary_path(path: &Path) -> PathBuf {
    path.with_extension(format!("json.tmp-{}", Uuid::new_v4()))
}

fn io_error(error: std::io::Error) -> SettingsStoreError {
    SettingsStoreError::Unavailable(error.to_string())
}

fn json_error(error: serde_json::Error) -> SettingsStoreError {
    SettingsStoreError::Invalid(error.to_string())
}

#[cfg(test)]
mod tests;
