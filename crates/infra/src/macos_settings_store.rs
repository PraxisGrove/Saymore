use std::{
    fs,
    fs::{File, OpenOptions, Permissions},
    io::{BufReader, BufWriter, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process,
    sync::{Mutex, MutexGuard},
};

use template_app::{
    AsrSettings, LlmSettings, OpenAiCompatibleAsrSettings, ProviderCatalog, ProviderConfigStore,
    ProviderDataConsent, ProviderInstance, SaymoreSettings, SettingsStore, SettingsStoreError,
    VolcengineAsrSettings,
};
use uuid::Uuid;

use crate::app_paths::{AppEnvironment, AppPaths};

mod schema;

use schema::{LegacySettings, StoredCatalog, catalog_to_settings, legacy_catalog};

const CONFIG_VERSION: u32 = 3;
const VOLCENGINE_TYPE: &str = "volcengine";
const OPENAI_TRANSCRIPTIONS_TYPE: &str = "openai_transcriptions";
const CHAT_COMPLETIONS_TYPE: &str = "openai_compatible";
const LLM_DATA_SCOPE: &str =
    "transcript+confirmed_dictionary_terms+local_correction_fragment+refinement_parameters:v2";

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
        provider.data_consent = Some(ProviderDataConsent {
            fingerprint: endpoint_fingerprint(expected_base_url),
        });
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
        fs::set_permissions(parent, Permissions::from_mode(0o700)).map_err(io_error)?;

        let temporary = temporary_path(&self.path);
        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(io_error)?;
        let mut writer = BufWriter::new(file);
        serde_json::to_writer_pretty(&mut writer, &StoredCatalog::from(catalog))
            .map_err(json_error)?;
        writer.write_all(b"\n").map_err(io_error)?;
        writer.flush().map_err(io_error)?;
        writer.get_ref().sync_all().map_err(io_error)?;
        fs::rename(&temporary, &self.path).map_err(io_error)?;
        fs::set_permissions(&self.path, Permissions::from_mode(0o600)).map_err(io_error)?;
        Ok(())
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

fn update_asr_providers(catalog: &mut ProviderCatalog, settings: &AsrSettings) {
    update_volcengine_asr_provider(catalog, &settings.volcengine);
    update_openai_asr_provider(catalog, &settings.openai_compatible);
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
    provider.config = serde_json::json!({
        "base_url": config.base_url,
        "api_key": config.api_key,
        "model": config.model,
        "custom_headers": config.custom_headers
    });
    provider.data_consent = (!settings.confirmed_base_url.is_empty()
        && settings.confirmed_base_url == config.base_url)
        .then(|| ProviderDataConsent {
            fingerprint: endpoint_fingerprint(&config.base_url),
        });
    catalog.active.llm = settings.enabled.then(|| provider.id.clone());
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
    path.with_extension(format!("json.tmp-{}", process::id()))
}

fn io_error(error: std::io::Error) -> SettingsStoreError {
    SettingsStoreError::Unavailable(error.to_string())
}

fn json_error(error: serde_json::Error) -> SettingsStoreError {
    SettingsStoreError::Invalid(error.to_string())
}

#[cfg(test)]
mod tests;
