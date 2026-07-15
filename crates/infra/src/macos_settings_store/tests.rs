use std::{
    collections::BTreeMap,
    env,
    sync::atomic::{AtomicUsize, Ordering},
};

use super::*;
use template_app::LlmProviderPreset;

static TEST_ID: AtomicUsize = AtomicUsize::new(0);

#[test]
fn saves_and_loads_volcengine_settings_with_private_permissions() {
    let directory = test_directory();
    let path = directory.join("config.json");
    let store = JsonSettingsStore::at_path(path.clone());
    let settings = SaymoreSettings {
        asr: AsrSettings {
            volcengine: VolcengineAsrSettings {
                enabled: true,
                api_key: "test-key".to_owned(),
                model: "test-model".to_owned(),
            },
            ..AsrSettings::default()
        },
        llm: LlmSettings {
            enabled: true,
            confirmed_base_url: "https://llm.example/v1".to_owned(),
            chat_completions: ChatCompletionsLlmSettings {
                base_url: "https://llm.example/v1".to_owned(),
                api_key: "llm-test-key".to_owned(),
                model: "test-llm".to_owned(),
                custom_headers: BTreeMap::from([("X-Tenant".to_owned(), "tenant-a".to_owned())]),
            },
        },
    };

    assert!(store.save(&settings).is_ok());
    assert_eq!(Ok(settings), store.load());
    let Ok(metadata) = fs::metadata(&path) else {
        panic!("saved settings should have metadata");
    };
    assert_eq!(0o600, metadata.permissions().mode() & 0o777);
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn migrates_version_one_settings_with_default_llm_configuration() {
    let directory = test_directory();
    let path = directory.join("config.json");
    assert!(fs::create_dir_all(&directory).is_ok());
    assert!(
        fs::write(
            &path,
            r#"{
                "version": 1,
                "asr": {
                    "volcengine": {
                        "enabled": true,
                        "api_key": "existing-key",
                        "model": "existing-model"
                    }
                }
            }"#,
        )
        .is_ok()
    );
    let store = JsonSettingsStore::at_path(path.clone());
    let settings = store.load();
    assert_eq!(
        Ok(SaymoreSettings {
            asr: AsrSettings {
                volcengine: VolcengineAsrSettings {
                    enabled: true,
                    api_key: "existing-key".to_owned(),
                    model: "existing-model".to_owned(),
                },
                ..AsrSettings::default()
            },
            llm: LlmSettings::default(),
        }),
        settings
    );
    let Ok(migrated) = fs::read_to_string(&path) else {
        panic!("migrated provider config should remain readable");
    };
    let Ok(migrated): Result<serde_json::Value, _> = serde_json::from_str(&migrated) else {
        panic!("migrated provider config should be valid JSON");
    };
    assert_eq!(Some(3), migrated["version"].as_u64());
    assert_eq!(Some(1), migrated["asr_providers"].as_array().map(Vec::len));
    let first_id = migrated["asr_providers"][0]["id"].clone();
    assert_eq!(settings, store.load());
    let Ok(reloaded): Result<serde_json::Value, _> = fs::read_to_string(&path)
        .map_err(serde_json::Error::io)
        .and_then(|contents| serde_json::from_str(&contents))
    else {
        panic!("reloaded provider config should be valid JSON");
    };
    assert_eq!(first_id, reloaded["asr_providers"][0]["id"]);
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn keeps_an_inactive_legacy_llm_configuration_available() {
    let directory = test_directory();
    let path = directory.join("config.json");
    assert!(fs::create_dir_all(&directory).is_ok());
    assert!(
        fs::write(
            &path,
            r#"{
                "version": 2,
                "llm": {
                    "enabled": false,
                    "confirmed_base_url": "https://token.sensenova.cn/v1",
                    "chat_completions": {
                        "base_url": "https://token.sensenova.cn/v1",
                        "api_key": "test-key",
                        "model": "sensenova-6.7-flash-lite"
                    }
                }
            }"#,
        )
        .is_ok()
    );
    let store = JsonSettingsStore::at_path(path);

    let Ok(settings) = store.load() else {
        panic!("inactive LLM settings should remain readable after migration");
    };

    assert!(!settings.llm.enabled);
    assert_eq!(
        "https://token.sensenova.cn/v1",
        settings.llm.chat_completions.base_url
    );
    assert_eq!(
        "sensenova-6.7-flash-lite",
        settings.llm.chat_completions.model
    );
    assert_eq!("test-key", settings.llm.chat_completions.api_key);
    assert_eq!(
        "https://token.sensenova.cn/v1",
        settings.llm.confirmed_base_url
    );

    let _ = fs::remove_dir_all(directory);
}

#[test]
fn round_trips_multiple_and_unknown_provider_instances() {
    let directory = test_directory();
    let path = directory.join("config.json");
    let store = JsonSettingsStore::at_path(path);
    let catalog = ProviderCatalog {
        active: ActiveProviders {
            asr: Some("asr-secondary".to_owned()),
            llm: None,
        },
        asr_providers: vec![
            ProviderInstance {
                id: "asr-primary".to_owned(),
                name: "Volcengine primary".to_owned(),
                provider_type: "volcengine".to_owned(),
                config: serde_json::json!({"api_key": "one", "model": "m1"}),
                data_consent: None,
            },
            ProviderInstance {
                id: "asr-secondary".to_owned(),
                name: "Future ASR".to_owned(),
                provider_type: "future_asr".to_owned(),
                config: serde_json::json!({
                    "endpoint": "https://asr.example/v2",
                    "future_option": {"mode": "lossless"}
                }),
                data_consent: None,
            },
        ],
        llm_providers: vec![ProviderInstance {
            id: "llm-one".to_owned(),
            name: "OpenAI compatible".to_owned(),
            provider_type: "openai_compatible".to_owned(),
            config: serde_json::json!({"base_url": "https://llm.example/v1", "model": "m2"}),
            data_consent: Some(ProviderDataConsent {
                fingerprint: "endpoint:https://llm.example/v1".to_owned(),
            }),
        }],
    };

    assert_eq!(Ok(()), store.save_catalog(&catalog));
    assert_eq!(Ok(catalog), store.load_catalog());
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn enables_only_the_provider_that_remains_selected_and_unchanged() {
    let directory = test_directory();
    let store = JsonSettingsStore::at_path(directory.join("config.json"));
    let mut catalog = ProviderCatalog::default();
    catalog.save_llm_provider_config(LlmProviderPreset::SenseNova, "sense-key");
    catalog.save_llm_provider_config(LlmProviderPreset::DeepSeek, "deepseek-key");
    catalog.select_llm_provider(LlmProviderPreset::SenseNova);
    assert_eq!(Ok(()), store.save_catalog(&catalog));

    catalog.select_llm_provider(LlmProviderPreset::DeepSeek);
    assert_eq!(Ok(()), store.save_catalog(&catalog));

    assert_eq!(
        Ok(false),
        store.enable_llm_provider_if_unchanged(
            LlmProviderPreset::SenseNova,
            "sensenova",
            "sense-key"
        )
    );
    assert_eq!(
        Ok(true),
        store.enable_llm_provider_if_unchanged(
            LlmProviderPreset::DeepSeek,
            "deepseek",
            "deepseek-key"
        )
    );
    let Ok(settings) = store.load() else {
        panic!("enabled DeepSeek settings should remain readable");
    };
    assert!(settings.llm.enabled);
    assert_eq!(
        LlmProviderPreset::DeepSeek.base_url(),
        settings.llm.confirmed_base_url
    );
    assert_eq!("deepseek-key", settings.llm.chat_completions.api_key);
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn rejects_active_provider_from_the_wrong_partition() {
    let directory = test_directory();
    let store = JsonSettingsStore::at_path(directory.join("config.json"));
    let catalog = ProviderCatalog {
        active: ActiveProviders {
            asr: Some("llm-only".to_owned()),
            llm: None,
        },
        asr_providers: Vec::new(),
        llm_providers: vec![ProviderInstance {
            id: "llm-only".to_owned(),
            name: "LLM".to_owned(),
            provider_type: "openai_compatible".to_owned(),
            config: serde_json::json!({}),
            data_consent: None,
        }],
    };

    assert!(matches!(
        store.save_catalog(&catalog),
        Err(SettingsStoreError::Invalid(_))
    ));
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn legacy_endpoint_only_consent_does_not_enable_llm() {
    let directory = test_directory();
    let path = directory.join("config.json");
    assert!(fs::create_dir_all(&directory).is_ok());
    assert!(
        fs::write(
            &path,
            r#"{
                "version": 3,
                "active": {"llm": "llm-one"},
                "llm_providers": [{
                    "id": "llm-one",
                    "name": "LLM",
                    "type": "openai_compatible",
                    "config": {
                        "base_url": "https://llm.example/v1",
                        "api_key": "key",
                        "model": "model"
                    },
                    "data_consent": {"fingerprint": "endpoint:https://llm.example/v1"}
                }]
            }"#,
        )
        .is_ok()
    );

    let store = JsonSettingsStore::at_path(path);
    let Ok(settings) = store.load() else {
        panic!("provider settings should remain readable");
    };
    assert!(!settings.llm.enabled);
    assert!(settings.llm.confirmed_base_url.is_empty());
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn rejects_unknown_config_versions() {
    let directory = test_directory();
    let path = directory.join("config.json");
    assert!(fs::create_dir_all(&directory).is_ok());
    assert!(fs::write(&path, r#"{"version":99,"asr":{}}"#).is_ok());
    let store = JsonSettingsStore::at_path(path);
    assert!(matches!(store.load(), Err(SettingsStoreError::Invalid(_))));
    let _ = fs::remove_dir_all(directory);
}

fn test_directory() -> PathBuf {
    let id = TEST_ID.fetch_add(1, Ordering::Relaxed);
    env::temp_dir().join(format!("saymore-settings-{}-{id}", process::id()))
}
