use std::collections::BTreeMap;

use serde::Deserialize;
use template_app::{LlmProviderPreset, ProviderCatalog, SettingsStoreError};

/// A Magpie Agent plugin connection, imported only when the settings load.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MagpieConnection {
    base_url: String,
    #[serde(default)]
    api_key: String,
    model: String,
    profile: String,
}

pub(super) fn import(
    catalog: &mut ProviderCatalog,
    shared: &mut BTreeMap<String, serde_json::Value>,
) -> Result<bool, SettingsStoreError> {
    let Some(value) = shared
        .get("integrations")
        .and_then(serde_json::Value::as_object)
        .and_then(|integrations| integrations.get("magpie"))
    else {
        return Ok(false);
    };
    let connection: MagpieConnection = serde_json::from_value(value.clone()).map_err(|error| {
        SettingsStoreError::Invalid(format!("invalid Magpie connection: {error}"))
    })?;
    let model = connection.model.trim();
    if model.is_empty()
        || connection.base_url.trim_end_matches('/') != LlmProviderPreset::Magpie.base_url()
        || connection.profile != LlmProviderPreset::Magpie.profile().chat_completions.id()
    {
        return Err(SettingsStoreError::Invalid(
            "Magpie connection does not match Saymore's local gateway".to_owned(),
        ));
    }
    let changed = match catalog
        .llm_providers
        .iter()
        .find(|provider| provider.id == LlmProviderPreset::Magpie.id())
    {
        Some(provider) => {
            let model_changed = provider
                .config
                .get("model")
                .and_then(serde_json::Value::as_str)
                != Some(model);
            let credentials_changed = provider
                .config
                .get("api_key")
                .and_then(serde_json::Value::as_str)
                != Some(connection.api_key.as_str());
            model_changed || credentials_changed
        }
        None => true,
    };
    if changed {
        for provider in &mut catalog.llm_providers {
            if provider.id == LlmProviderPreset::Magpie.id() {
                provider.data_consent = None;
            }
        }
        // A changed active Magpie connection must be tested again before it
        // can submit text. Importing never switches to Magpie on its own.
        let previous_selection = catalog
            .active
            .llm
            .clone()
            .filter(|id| id != LlmProviderPreset::Magpie.id());
        catalog.save_llm_provider_model_config(
            LlmProviderPreset::Magpie,
            &connection.api_key,
            model,
        );
        catalog.active.llm = previous_selection;
        if let Some(provider) = catalog
            .llm_providers
            .iter_mut()
            .find(|provider| provider.id == LlmProviderPreset::Magpie.id())
            && let Some(config) = provider.config.as_object_mut()
        {
            config.insert("enabled".to_owned(), serde_json::Value::Bool(false));
        }
    }
    if let Some(integrations) = shared
        .get_mut("integrations")
        .and_then(serde_json::Value::as_object_mut)
    {
        integrations.remove("magpie");
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use template_app::{LlmProviderPreset, ProviderConfigStore, SettingsStore};

    use super::super::JsonSettingsStore;
    use super::super::tests::test_directory;
    use super::*;

    fn attach_value(directory: &std::path::Path, magpie: serde_json::Value) {
        let path = directory.join("config.json");
        let contents = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error) => panic!("read test config: {error}"),
        };
        let mut settings = match serde_json::from_str::<serde_json::Value>(&contents) {
            Ok(settings) => settings,
            Err(error) => panic!("parse test config: {error}"),
        };
        if let Some(root) = settings.as_object_mut() {
            root.insert(
                "integrations".to_owned(),
                serde_json::json!({
                    "magpie": magpie,
                    "another-application": {"keep": true},
                }),
            );
        }
        let contents = match serde_json::to_string_pretty(&settings) {
            Ok(contents) => contents,
            Err(error) => panic!("serialize test config: {error}"),
        };
        if fs::write(path, contents).is_err() {
            panic!("attach Magpie test connection");
        }
    }

    fn handoff(model: &str) -> serde_json::Value {
        serde_json::json!({
            "base_url": "http://127.0.0.1:3425/v1",
            "api_key": "magpie-test-key",
            "model": model,
            "profile": "portable"
        })
    }

    fn attach(directory: &std::path::Path, model: &str) {
        attach_value(directory, handoff(model));
    }

    #[test]
    fn changing_an_active_connection_disables_it_until_the_user_tests_it() {
        let directory = test_directory();
        let store = JsonSettingsStore::at_path(directory.join("config.json"));
        let mut catalog = ProviderCatalog::default();
        catalog.save_llm_provider_model_config(LlmProviderPreset::Magpie, "old-key", "old/model");
        catalog.select_llm_provider(LlmProviderPreset::Magpie);
        catalog.llm_providers[0].data_consent = None;
        assert!(store.save_catalog(&catalog).is_ok());
        let Ok(mut settings) = store.load() else {
            panic!("test settings should be readable");
        };
        settings.llm.enabled = true;
        settings.llm.confirmed_base_url = LlmProviderPreset::Magpie.base_url().to_owned();
        assert!(store.save(&settings).is_ok());

        attach(&directory, "new/model");
        assert!(store.ensure_exists().is_ok());
        let Ok(settings) = store.load() else {
            panic!("changed Magpie connection should be readable");
        };
        assert!(!settings.llm.enabled);
        assert!(settings.llm.confirmed_base_url.is_empty());

        attach(&directory, "new/model");
        assert!(store.ensure_exists().is_ok());
        let Ok(repeated) = store.load() else {
            panic!("repeated handoff should be readable");
        };
        assert_eq!(settings, repeated);
        assert!(store.save(&repeated).is_ok());
        assert!(store.ensure_exists().is_ok());
        let Ok(first_saved) = fs::read_to_string(directory.join("config.json")) else {
            panic!("saved config should be readable");
        };
        let Ok(first_saved) = serde_json::from_str::<serde_json::Value>(&first_saved) else {
            panic!("saved config should be valid JSON");
        };
        assert_eq!(
            Some(true),
            first_saved
                .get("integrations")
                .and_then(|integrations| integrations.get("another-application"))
                .and_then(|value| value.get("keep"))
                .and_then(serde_json::Value::as_bool)
        );
        assert!(
            first_saved
                .get("integrations")
                .and_then(|integrations| integrations.get("magpie"))
                .is_none()
        );
        assert!(store.save_catalog(&catalog).is_ok());
        assert!(store.ensure_exists().is_ok());
        let Ok(second_saved) = fs::read_to_string(directory.join("config.json")) else {
            panic!("provider save should preserve extension fields");
        };
        let Ok(second_saved) = serde_json::from_str::<serde_json::Value>(&second_saved) else {
            panic!("saved config should be valid JSON");
        };
        assert!(
            second_saved
                .get("integrations")
                .is_some_and(|integrations| {
                    integrations.get("another-application").is_some()
                        && integrations.get("magpie").is_none()
                })
        );
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn invalid_connections_do_not_replace_existing_providers() {
        let directory = test_directory();
        let store = JsonSettingsStore::at_path(directory.join("config.json"));
        let catalog = ProviderCatalog::default();
        assert!(store.save_catalog(&catalog).is_ok());
        attach_value(
            &directory,
            serde_json::json!({
                "base_url": "https://example.com/v1",
                "api_key": "untrusted-key",
                "model": "provider/model",
                "profile": "portable"
            }),
        );

        assert!(store.load_catalog().is_err());
        let Ok(persisted) = fs::read_to_string(directory.join("config.json")) else {
            panic!("rejected config should remain available for Magpie to restore");
        };
        assert!(persisted.contains("example.com"));
        assert_eq!(catalog.llm_providers.len(), 0);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn imports_magpie_without_replacing_custom_or_enabling_polishing() {
        let directory = test_directory();
        let store = JsonSettingsStore::at_path(directory.join("config.json"));
        let mut catalog = ProviderCatalog::default();
        catalog.save_custom_llm_provider_config(
            "http://127.0.0.1:3425/v1",
            "custom-key",
            "custom-model",
        );
        catalog.select_llm_provider(LlmProviderPreset::Custom);
        assert!(store.save_catalog(&catalog).is_ok());
        attach(&directory, "provider/model");

        assert!(
            store.save_catalog(&catalog).is_ok(),
            "external-edit validation"
        );
        let before_load = fs::read_to_string(directory.join("config.json"));
        assert!(before_load.is_ok_and(|contents| contents.contains("provider/model")));

        assert!(store.ensure_exists().is_ok());
        let catalog = match store.load_catalog() {
            Ok(catalog) => catalog,
            Err(error) => panic!("read imported settings: {error}"),
        };
        assert!(matches!(
            catalog.active_llm_provider(),
            Some(LlmProviderPreset::Custom)
        ));
        assert_eq!(
            Some("provider/model"),
            catalog.configured_llm_provider_model(LlmProviderPreset::Magpie)
        );
        assert_eq!(
            Some("custom-model"),
            catalog.configured_llm_provider_model(LlmProviderPreset::Custom)
        );
        let settings = match store.load() {
            Ok(settings) => settings,
            Err(error) => panic!("read polishing state: {error}"),
        };
        assert!(!settings.llm.enabled);
        assert!(settings.llm.confirmed_base_url.is_empty());
        let contents = match fs::read_to_string(directory.join("config.json")) {
            Ok(contents) => contents,
            Err(error) => panic!("read persisted settings: {error}"),
        };
        let Ok(persisted) = serde_json::from_str::<serde_json::Value>(&contents) else {
            panic!("saved config should be valid JSON");
        };
        assert!(persisted.get("integrations").is_some_and(|integrations| {
            integrations.get("another-application").is_some()
                && integrations.get("magpie").is_none()
        }));
        assert!(store.ensure_exists().is_ok());
        let _ = fs::remove_dir_all(directory);
    }
}
