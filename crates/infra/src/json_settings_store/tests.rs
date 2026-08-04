use std::{
    collections::BTreeMap,
    env, process,
    sync::atomic::{AtomicUsize, Ordering},
};

use super::*;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use template_app::{ActiveProviders, ChatCompletionsLlmSettings, LlmProviderPreset};

static TEST_ID: AtomicUsize = AtomicUsize::new(0);

#[test]
fn provider_configuration_store_saves_asr_without_changing_the_active_provider() {
    let directory = test_directory();
    let store = JsonSettingsStore::at_path(directory.join("config.json"));
    let mut catalog = ProviderCatalog::default();
    catalog.select_macos_speech_provider();
    assert!(store.save_catalog(&catalog).is_ok());
    let candidate = AsrProviderConfiguration::Volcengine(VolcengineAsrSettings {
        enabled: true,
        api_key: "candidate-key".to_owned(),
        model: "candidate-model".to_owned(),
    });

    assert!(store.save_asr_configuration(&candidate).is_ok());
    let Ok(saved) = store.load_catalog() else {
        panic!("saved Provider catalog should remain readable");
    };
    assert!(saved.macos_speech_is_active());
    assert_eq!(
        Some("candidate-key"),
        saved
            .asr_providers
            .iter()
            .find(|provider| provider.provider_type == "volcengine")
            .and_then(|provider| provider.config.get("api_key"))
            .and_then(serde_json::Value::as_str)
    );
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn provider_configuration_store_atomically_saves_selects_and_enables_llm() {
    let directory = test_directory();
    let store = JsonSettingsStore::at_path(directory.join("config.json"));
    let candidate = LlmProviderConfiguration::new(
        LlmProviderPreset::DeepSeek,
        ChatCompletionsLlmSettings {
            model: "deepseek-chat".to_owned(),
            ..LlmProviderPreset::DeepSeek.settings("candidate-key")
        },
    );

    assert!(store.save_and_enable_llm_configuration(&candidate).is_ok());
    let Ok(settings) = store.load() else {
        panic!("saved LLM configuration should remain readable");
    };
    assert!(settings.llm.enabled);
    assert_eq!(
        LlmProviderPreset::DeepSeek.base_url(),
        settings.llm.confirmed_base_url
    );
    assert_eq!("candidate-key", settings.llm.chat_completions.api_key);
    assert_eq!("deepseek-chat", settings.llm.chat_completions.model);
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn saves_and_loads_volcengine_settings_with_private_permissions() {
    let directory = test_directory();
    let path = directory.join("config.json");
    let store = JsonSettingsStore::at_path(path);
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
                profile: Default::default(),
            },
        },
    };

    assert!(store.save(&settings).is_ok());
    assert_eq!(Ok(settings), store.load());
    #[cfg(unix)]
    let Ok(metadata) = fs::metadata(&store.path) else {
        panic!("saved settings should have metadata");
    };
    #[cfg(unix)]
    assert_eq!(0o600, metadata.permissions().mode() & 0o777);
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn a_second_save_atomically_replaces_the_complete_document() {
    let directory = test_directory();
    let path = directory.join("config.json");
    let store = JsonSettingsStore::at_path(path.clone());
    let mut catalog = ProviderCatalog::default();
    catalog.save_llm_provider_config(LlmProviderPreset::SenseNova, "first-key");
    assert_eq!(Ok(()), store.save_catalog(&catalog));

    catalog.save_llm_provider_config(LlmProviderPreset::SenseNova, "second-key");
    assert_eq!(Ok(()), store.save_catalog(&catalog));

    let Ok(document) = fs::read_to_string(path) else {
        panic!("replaced provider config should remain readable");
    };
    let Ok(value): Result<serde_json::Value, _> = serde_json::from_str(&document) else {
        panic!("replaced provider config should contain complete JSON");
    };
    assert_eq!(Some(3), value["version"].as_u64());
    assert!(document.contains("second-key"));
    assert!(!document.contains("first-key"));
    assert_eq!(Ok(catalog), store.load_catalog());
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn model_catalog_round_trips_with_selection_timestamp_and_scope() {
    let directory = test_directory();
    let store = JsonSettingsStore::at_path(directory.join("config.json"));
    let mut catalog = ProviderCatalog::default();
    catalog.select_llm_provider(LlmProviderPreset::DeepSeek);
    assert_eq!(Ok(()), store.save_catalog(&catalog));

    assert_eq!(
        Ok(true),
        store.cache_llm_model_catalog(
            LlmProviderPreset::DeepSeek,
            LlmProviderPreset::DeepSeek.model_list_url(),
            ChatCompletionsProfile::DeepSeek,
            vec!["deepseek-chat".to_owned(), "deepseek-reasoner".to_owned()],
            "deepseek-reasoner",
            1_234,
        )
    );

    let cached = store.load_catalog().ok().and_then(|catalog| {
        catalog.llm_model_catalog(
            LlmProviderPreset::DeepSeek,
            LlmProviderPreset::DeepSeek.model_list_url(),
            ChatCompletionsProfile::DeepSeek,
        )
    });
    assert_eq!(
        Some(vec![
            "deepseek-chat".to_owned(),
            "deepseek-reasoner".to_owned()
        ]),
        cached.as_ref().map(|catalog| catalog.models.clone())
    );
    assert_eq!(
        Some("deepseek-reasoner"),
        cached
            .as_ref()
            .map(|catalog| catalog.selected_model.as_str())
    );
    assert_eq!(Some(1_234), cached.map(|catalog| catalog.refreshed_at_ms));
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn failed_model_catalog_refresh_preserves_the_persisted_catalog() {
    let directory = test_directory();
    let store = JsonSettingsStore::at_path(directory.join("config.json"));
    let mut catalog = ProviderCatalog::default();
    catalog.select_llm_provider(LlmProviderPreset::DeepSeek);
    assert_eq!(Ok(()), store.save_catalog(&catalog));
    assert_eq!(
        Ok(true),
        store.cache_llm_model_catalog(
            LlmProviderPreset::DeepSeek,
            LlmProviderPreset::DeepSeek.model_list_url(),
            ChatCompletionsProfile::DeepSeek,
            vec!["deepseek-chat".to_owned(), "deepseek-reasoner".to_owned()],
            "deepseek-reasoner",
            1_234,
        )
    );

    assert_eq!(
        Ok(false),
        store.cache_llm_model_catalog(
            LlmProviderPreset::DeepSeek,
            LlmProviderPreset::DeepSeek.model_list_url(),
            ChatCompletionsProfile::DeepSeek,
            Vec::new(),
            "",
            9_999,
        )
    );

    let cached = store.load_catalog().ok().and_then(|catalog| {
        catalog.llm_model_catalog(
            LlmProviderPreset::DeepSeek,
            LlmProviderPreset::DeepSeek.model_list_url(),
            ChatCompletionsProfile::DeepSeek,
        )
    });
    assert_eq!(
        Some(vec![
            "deepseek-chat".to_owned(),
            "deepseek-reasoner".to_owned()
        ]),
        cached.as_ref().map(|catalog| catalog.models.clone())
    );
    assert_eq!(
        Some("deepseek-reasoner"),
        cached
            .as_ref()
            .map(|catalog| catalog.selected_model.as_str())
    );
    assert_eq!(Some(1_234), cached.map(|catalog| catalog.refreshed_at_ms));
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn cached_model_selection_updates_provider_without_losing_enablement() {
    let directory = test_directory();
    let store = JsonSettingsStore::at_path(directory.join("config.json"));
    let mut catalog = ProviderCatalog::default();
    catalog.save_llm_provider_model_config(LlmProviderPreset::Kimi, "kimi-key", "kimi-k2.5");
    catalog.select_llm_provider(LlmProviderPreset::Kimi);
    assert!(catalog.cache_llm_model_catalog(
        LlmProviderPreset::Kimi,
        LlmProviderPreset::Kimi.model_list_url(),
        ChatCompletionsProfile::Kimi,
        vec!["kimi-k2.5".to_owned(), "kimi-k2.6".to_owned()],
        "kimi-k2.5",
        1_234,
    ));
    assert_eq!(Ok(()), store.save_catalog(&catalog));
    assert_eq!(
        Ok(true),
        store.enable_llm_provider_if_unchanged(
            LlmProviderPreset::Kimi.id(),
            LlmProviderPreset::Kimi.base_url(),
            "kimi-key",
        )
    );

    assert_eq!(
        Ok(true),
        store.select_cached_llm_model(
            LlmProviderPreset::Kimi,
            LlmProviderPreset::Kimi.model_list_url(),
            ChatCompletionsProfile::Kimi,
            "kimi-k2.6",
        )
    );
    let Ok(settings) = store.load() else {
        panic!("updated Kimi settings should remain readable");
    };
    assert!(settings.llm.enabled);
    assert_eq!("kimi-k2.6", settings.llm.chat_completions.model);
    let cached = store.load_catalog().ok().and_then(|catalog| {
        catalog.llm_model_catalog(
            LlmProviderPreset::Kimi,
            LlmProviderPreset::Kimi.model_list_url(),
            ChatCompletionsProfile::Kimi,
        )
    });
    assert_eq!(
        Some("kimi-k2.6"),
        cached
            .as_ref()
            .map(|catalog| catalog.selected_model.as_str())
    );
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn saving_llm_enablement_preserves_the_cached_model_catalog() {
    let directory = test_directory();
    let store = JsonSettingsStore::at_path(directory.join("config.json"));
    let mut catalog = ProviderCatalog::default();
    catalog.save_llm_provider_config(LlmProviderPreset::DeepSeek, "saved-key");
    catalog.select_llm_provider(LlmProviderPreset::DeepSeek);
    assert!(catalog.cache_llm_model_catalog(
        LlmProviderPreset::DeepSeek,
        LlmProviderPreset::DeepSeek.model_list_url(),
        ChatCompletionsProfile::DeepSeek,
        vec!["deepseek-chat".to_owned(), "deepseek-reasoner".to_owned()],
        "deepseek-reasoner",
        5_678,
    ));
    assert_eq!(Ok(()), store.save_catalog(&catalog));

    let Ok(mut settings) = store.load() else {
        panic!("LLM settings should remain readable");
    };
    settings.llm.enabled = false;
    assert_eq!(Ok(()), store.save(&settings));

    let cached = store.load_catalog().ok().and_then(|catalog| {
        catalog.llm_model_catalog(
            LlmProviderPreset::DeepSeek,
            LlmProviderPreset::DeepSeek.model_list_url(),
            ChatCompletionsProfile::DeepSeek,
        )
    });
    assert_eq!(
        Some("deepseek-reasoner"),
        cached
            .as_ref()
            .map(|catalog| catalog.selected_model.as_str())
    );
    assert_eq!(Some(5_678), cached.map(|catalog| catalog.refreshed_at_ms));
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
            "sensenova",
            LlmProviderPreset::SenseNova.base_url(),
            "sense-key"
        )
    );
    assert_eq!(
        Ok(true),
        store.enable_llm_provider_if_unchanged(
            "deepseek",
            LlmProviderPreset::DeepSeek.base_url(),
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
fn disabling_llm_keeps_the_current_provider_available_for_reenable() {
    let directory = test_directory();
    let store = JsonSettingsStore::at_path(directory.join("config.json"));
    let mut catalog = ProviderCatalog::default();
    catalog.configure_volcengine_asr_provider(&VolcengineAsrSettings {
        enabled: true,
        api_key: "volcengine-key".to_owned(),
        model: "volc.seedasr.sauc.duration".to_owned(),
    });
    assert!(catalog.select_volcengine_asr_provider());
    let expected_asr = catalog.active.asr.clone();
    catalog.save_llm_provider_config(LlmProviderPreset::DeepSeek, "deepseek-key");
    catalog.select_llm_provider(LlmProviderPreset::DeepSeek);
    assert_eq!(Ok(()), store.save_catalog(&catalog));
    assert_eq!(
        Ok(true),
        store.enable_llm_provider_if_unchanged(
            LlmProviderPreset::DeepSeek.id(),
            LlmProviderPreset::DeepSeek.base_url(),
            "deepseek-key"
        )
    );

    let Ok(mut settings) = store.load() else {
        panic!("enabled DeepSeek settings should be readable");
    };
    settings.llm.enabled = false;
    assert_eq!(Ok(()), store.save(&settings));

    let Ok(catalog) = store.load_catalog() else {
        panic!("disabled provider catalog should be readable");
    };
    assert_eq!(
        Some(LlmProviderPreset::DeepSeek),
        catalog.active_llm_provider()
    );
    assert_eq!(expected_asr, catalog.active.asr);
    let Ok(settings) = store.load() else {
        panic!("disabled DeepSeek settings should remain readable");
    };
    assert!(settings.asr.volcengine.enabled);
    assert_eq!("volcengine-key", settings.asr.volcengine.api_key);
    assert_eq!("volc.seedasr.sauc.duration", settings.asr.volcengine.model);
    assert!(!settings.llm.enabled);
    assert_eq!(
        LlmProviderPreset::DeepSeek.base_url(),
        settings.llm.chat_completions.base_url
    );
    assert_eq!(
        LlmProviderPreset::DeepSeek.base_url(),
        settings.llm.confirmed_base_url
    );

    assert_eq!(
        Ok(true),
        store.enable_llm_provider_if_unchanged(
            LlmProviderPreset::DeepSeek.id(),
            LlmProviderPreset::DeepSeek.base_url(),
            "deepseek-key"
        )
    );
    assert!(store.load().is_ok_and(|settings| settings.llm.enabled));
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn disabling_llm_preserves_the_selected_local_asr_provider() {
    let directory = test_directory();
    let store = JsonSettingsStore::at_path(directory.join("config.json"));
    let mut catalog = ProviderCatalog::default();
    catalog.configure_volcengine_asr_provider(&VolcengineAsrSettings {
        enabled: false,
        api_key: "volcengine-key".to_owned(),
        model: "volc.seedasr.sauc.duration".to_owned(),
    });
    catalog.select_paraformer_provider();
    catalog.save_llm_provider_config(LlmProviderPreset::DeepSeek, "deepseek-key");
    catalog.select_llm_provider(LlmProviderPreset::DeepSeek);
    assert_eq!(Ok(()), store.save_catalog(&catalog));
    assert_eq!(
        Ok(true),
        store.enable_llm_provider_if_unchanged(
            LlmProviderPreset::DeepSeek.id(),
            LlmProviderPreset::DeepSeek.base_url(),
            "deepseek-key"
        )
    );

    let Ok(mut settings) = store.load() else {
        panic!("enabled settings should be readable");
    };
    settings.llm.enabled = false;
    assert_eq!(Ok(()), store.save(&settings));

    let Ok(catalog) = store.load_catalog() else {
        panic!("disabled provider catalog should be readable");
    };
    assert!(catalog.paraformer_is_active());
    assert!(
        store
            .load()
            .is_ok_and(|settings| { !settings.llm.enabled && !settings.asr.volcengine.enabled })
    );
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn custom_llm_provider_round_trips_and_can_enable_without_a_local_api_key() {
    let directory = test_directory();
    let store = JsonSettingsStore::at_path(directory.join("config.json"));
    let mut catalog = ProviderCatalog::default();
    catalog.save_custom_llm_provider_config("http://localhost:11434/v1", "", "qwen3:8b");
    catalog.select_llm_provider(LlmProviderPreset::Custom);
    assert_eq!(Ok(()), store.save_catalog(&catalog));

    assert_eq!(
        Ok(true),
        store.enable_llm_provider_if_unchanged("custom", "http://localhost:11434/v1", "")
    );
    let Ok(settings) = store.load() else {
        panic!("custom LLM settings should remain readable");
    };
    assert!(settings.llm.enabled);
    assert_eq!(
        "http://localhost:11434/v1",
        settings.llm.chat_completions.base_url
    );
    assert_eq!("qwen3:8b", settings.llm.chat_completions.model);
    assert!(settings.llm.chat_completions.api_key.is_empty());
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn loads_the_existing_custom_asr_provider_schema() {
    let directory = test_directory();
    let path = directory.join("config.json");
    assert!(fs::create_dir_all(&directory).is_ok());
    assert!(
        fs::write(
            &path,
            r#"{
                "version": 3,
                "active": {"asr": "custom-asr"},
                "asr_providers": [{
                    "id": "custom-asr",
                    "name": "OpenAI-compatible ASR",
                    "type": "openai_transcriptions",
                    "config": {
                        "base_url": "https://asr.example/v1",
                        "api_key": "existing-key",
                        "model": "whisper-1"
                    }
                }]
            }"#,
        )
        .is_ok()
    );

    let store = JsonSettingsStore::at_path(path);
    let Ok(settings) = store.load() else {
        panic!("existing custom ASR settings should remain readable");
    };
    assert_eq!(
        OpenAiCompatibleAsrSettings {
            enabled: true,
            base_url: "https://asr.example/v1".to_owned(),
            api_key: "existing-key".to_owned(),
            model: "whisper-1".to_owned(),
        },
        settings.asr.openai_compatible
    );
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn persists_macos_speech_selection_without_removing_cloud_configuration() {
    let directory = test_directory();
    let store = JsonSettingsStore::at_path(directory.join("config.json"));
    let mut settings = SaymoreSettings::default();
    settings.asr.volcengine = VolcengineAsrSettings {
        enabled: true,
        api_key: "saved-key".to_owned(),
        model: "saved-model".to_owned(),
    };
    assert_eq!(Ok(()), store.save(&settings));
    let Ok(mut catalog) = store.load_catalog() else {
        panic!("saved provider catalog should be readable");
    };

    catalog.select_macos_speech_provider();
    assert_eq!(Ok(()), store.save_catalog(&catalog));

    let Ok(reloaded_catalog) = store.load_catalog() else {
        panic!("macOS Speech provider catalog should be readable");
    };
    let Ok(reloaded_settings) = store.load() else {
        panic!("cloud settings should remain readable");
    };
    assert!(reloaded_catalog.macos_speech_is_active());
    assert!(!reloaded_settings.asr.volcengine.enabled);
    assert_eq!("saved-key", reloaded_settings.asr.volcengine.api_key);
    assert_eq!("saved-model", reloaded_settings.asr.volcengine.model);

    let mut cloud_settings = reloaded_settings;
    cloud_settings.asr.volcengine.enabled = true;
    assert_eq!(Ok(()), store.save(&cloud_settings));
    let Ok(cloud_catalog) = store.load_catalog() else {
        panic!("reselected cloud provider catalog should be readable");
    };
    assert!(!cloud_catalog.macos_speech_is_active());
    assert_eq!(2, cloud_catalog.asr_providers.len());
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn persists_paraformer_selection_without_removing_cloud_configuration() {
    let directory = test_directory();
    let store = JsonSettingsStore::at_path(directory.join("config.json"));
    let mut catalog = ProviderCatalog::default();
    catalog.configure_volcengine_asr_provider(&VolcengineAsrSettings {
        enabled: false,
        api_key: "saved-key".to_owned(),
        model: "saved-model".to_owned(),
    });
    catalog.select_paraformer_provider();

    assert_eq!(Ok(()), store.save_catalog(&catalog));
    let Ok(reloaded) = store.load_catalog() else {
        panic!("Paraformer provider catalog should be readable");
    };

    assert!(reloaded.paraformer_is_active());
    assert_eq!(2, reloaded.asr_providers.len());
    assert_eq!(
        Some("saved-key"),
        reloaded.asr_providers[0]
            .config
            .get("api_key")
            .and_then(serde_json::Value::as_str)
    );
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn changing_only_llm_settings_preserves_macos_speech_selection() {
    let directory = test_directory();
    let store = JsonSettingsStore::at_path(directory.join("config.json"));
    let mut catalog = ProviderCatalog::default();
    catalog.configure_volcengine_asr_provider(&VolcengineAsrSettings {
        enabled: false,
        api_key: "saved-key".to_owned(),
        model: "volc.seedasr.sauc.duration".to_owned(),
    });
    catalog.select_macos_speech_provider();
    catalog.save_llm_provider_config(LlmProviderPreset::DeepSeek, "deepseek-key");
    catalog.select_llm_provider(LlmProviderPreset::DeepSeek);
    assert_eq!(Ok(()), store.save_catalog(&catalog));
    assert_eq!(
        Ok(true),
        store.enable_llm_provider_if_unchanged(
            LlmProviderPreset::DeepSeek.id(),
            LlmProviderPreset::DeepSeek.base_url(),
            "deepseek-key"
        )
    );

    let Ok(mut settings) = store.load() else {
        panic!("settings should be readable before disabling the LLM");
    };
    settings.llm.enabled = false;
    assert_eq!(Ok(()), store.save(&settings));

    let Ok(catalog) = store.load_catalog() else {
        panic!("provider catalog should be readable after disabling the LLM");
    };
    assert!(catalog.macos_speech_is_active());
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
