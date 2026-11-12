use super::*;

#[test]
fn retired_active_asr_is_cleared_and_persisted_without_fallback() {
    for (legacy_id, provider_index) in [
        ("local-paraformer", 0),
        ("legacy-streaming-instance", 0),
        ("local-sense-voice-small-int8", 1),
        ("legacy-sense-voice-instance", 2),
        ("local-moonshine", 3),
        ("legacy-moonshine-instance", 4),
    ] {
        let directory = test_directory();
        let path = directory.join("config.json");
        let mut document = retired_asr_document(legacy_id);
        document["asr_providers"][provider_index]["id"] = serde_json::json!(legacy_id);
        assert!(fs::create_dir_all(&directory).is_ok());
        assert!(fs::write(&path, document.to_string()).is_ok());
        let store = JsonSettingsStore::at_path(path.clone());

        let Ok((settings, catalog)) = store.load_settings_snapshot() else {
            panic!("retired active ASR {legacy_id} must load after normalization");
        };
        assert_eq!(None, catalog.active.asr);
        assert_eq!(Some("llm-one"), catalog.active.llm.as_deref());
        assert!(!settings.asr.volcengine.enabled);
        assert!(!settings.asr.openai_compatible.enabled);
        document["active"]["asr"] = serde_json::Value::Null;
        assert_eq!(document, read_document(&path));

        let Ok(persisted) = fs::read(&path) else {
            panic!("normalized configuration must be persisted");
        };
        let reopened = JsonSettingsStore::at_path(path.clone());
        assert_eq!(Ok(catalog.clone()), reopened.load_catalog());
        assert!(fs::read(&path).is_ok_and(|contents| contents == persisted));

        let mut retired_selection = catalog.clone();
        retired_selection.active.asr = Some(legacy_id.to_owned());
        assert_eq!(
            Err(SettingsStoreError::Invalid(
                "retired local ASR models cannot be selected".to_owned()
            )),
            reopened.save_catalog(&retired_selection)
        );
        assert_eq!(Ok(catalog), reopened.load_catalog());
        assert_eq!(document, read_document(&path));
        let _ = fs::remove_dir_all(directory);
    }
}

#[test]
fn inactive_retired_entries_preserve_the_current_asr_selection_and_document() {
    for active in [
        "local-zipformer",
        "local-seaco-paraformer",
        "volcengine-one",
        "custom-asr",
    ] {
        let directory = test_directory();
        let path = directory.join("config.json");
        let document = retired_asr_document(active);
        let original = document.to_string();
        assert!(fs::create_dir_all(&directory).is_ok());
        assert!(fs::write(&path, &original).is_ok());
        let store = JsonSettingsStore::at_path(path.clone());

        let Ok((settings, catalog)) = store.load_settings_snapshot() else {
            panic!("inactive legacy provider must not block current settings");
        };
        assert_eq!(Some(active), catalog.active.asr.as_deref());
        assert_eq!(active == "local-zipformer", catalog.zipformer_is_active());
        assert_eq!(
            active == "local-seaco-paraformer",
            catalog.seaco_is_active()
        );
        assert_eq!(active == "volcengine-one", settings.asr.volcengine.enabled);
        assert_eq!(
            active == "custom-asr",
            settings.asr.openai_compatible.enabled
        );
        assert!(fs::read_to_string(&path).is_ok_and(|contents| contents == original));

        assert_eq!(Ok(()), store.save_catalog(&catalog));
        assert_eq!(document, read_document(&path));
        assert_eq!(Ok(catalog), store.load_catalog());
        let _ = fs::remove_dir_all(directory);
    }
}

#[test]
fn a_retired_canonical_selection_is_cleared_even_if_its_entry_is_missing() {
    let directory = test_directory();
    let path = directory.join("config.json");
    let mut document = retired_asr_document("local-paraformer");
    let Some(providers) = document["asr_providers"].as_array_mut() else {
        panic!("fixture must contain ASR providers");
    };
    providers.remove(0);
    assert!(fs::create_dir_all(&directory).is_ok());
    assert!(fs::write(&path, document.to_string()).is_ok());
    let store = JsonSettingsStore::at_path(path.clone());

    assert!(
        store
            .load_catalog()
            .is_ok_and(|catalog| catalog.active.asr.is_none())
    );
    document["active"]["asr"] = serde_json::Value::Null;
    assert_eq!(document, read_document(&path));
    let _ = fs::remove_dir_all(directory);
}

fn read_document(path: &Path) -> serde_json::Value {
    let Ok(contents) = fs::read(path) else {
        panic!("provider configuration must remain readable");
    };
    let Ok(document) = serde_json::from_slice(&contents) else {
        panic!("provider configuration must remain valid JSON");
    };
    document
}

fn retired_asr_document(active: &str) -> serde_json::Value {
    serde_json::json!({
        "version": 3,
        "active": {"asr": active, "llm": "llm-one"},
        "asr_providers": [
            {
                "id": "local-paraformer", "name": "Paraformer", "type": "local_paraformer",
                "config": {"punctuation_mode": "local", "saved_option": "preserved"},
                "data_consent": null
            },
            {
                "id": "local-sense-voice-small-int8", "name": "SenseVoiceSmall",
                "type": "local_sense_voice", "config": {"saved_option": "preserved"},
                "data_consent": null
            },
            {
                "id": "legacy-sense-voice-instance", "name": "SenseVoiceSmall",
                "type": "local_sense_voice", "config": {}, "data_consent": null
            },
            {
                "id": "local-moonshine", "name": "Moonshine", "type": "moonshine",
                "config": {}, "data_consent": null
            },
            {
                "id": "legacy-moonshine-instance", "name": "Moonshine", "type": "moonshine",
                "config": {}, "data_consent": null
            },
            {
                "id": "local-seaco-paraformer", "name": "SeACo-Paraformer",
                "type": "seaco-paraformer", "config": {"local_option": true},
                "data_consent": null
            },
            {
                "id": "local-zipformer", "name": "Zipformer", "type": "zipformer",
                "config": {}, "data_consent": null
            },
            {
                "id": "volcengine-one", "name": "Volcengine", "type": "volcengine",
                "config": {"api_key": "saved-cloud-key", "model": "saved-model"},
                "data_consent": null
            },
            {
                "id": "custom-asr", "name": "Custom ASR", "type": "openai_transcriptions",
                "config": {"api_key": "compatible-key", "model": "whisper-1",
                    "base_url": "https://asr.example/v1"},
                "data_consent": null
            },
            {
                "id": "future-asr", "name": "Future", "type": "future_asr",
                "config": {"opaque": ["keep", {"nested": true}]}, "data_consent": null
            }
        ],
        "llm_providers": [{
            "id": "llm-one", "name": "LLM", "type": "openai_compatible",
            "config": {"base_url": "https://llm.example/v1", "api_key": "saved-llm-key",
                "model": "saved-llm", "enabled": false,
                "model_catalogs": [{"opaque": "preserved"}]},
            "data_consent": {"fingerprint": "saved-consent"}
        }]
    })
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
