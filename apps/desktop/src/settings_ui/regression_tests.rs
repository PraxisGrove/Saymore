use std::fs;

use template_app::{
    LlmProviderPreset, OpenAiCompatibleAsrSettings, ProviderCatalog, ProviderConfigStore,
    SaymoreSettings, SettingsStore, VolcengineAsrSettings,
};
use template_infra::JsonSettingsStore;
use uuid::Uuid;

use super::{
    AsrConfigError, asr_available_on_home, clear_asr_configuration, llm_consent_required,
    persist_llm_consent, save_asr_configuration, save_custom_asr_configuration,
    volcengine_api_key_is_valid,
};

const VALID_ASR_API_KEY: &str = "123e4567-e89b-42d3-a456-426614174000";

#[test]
fn configured_asr_remains_available_on_home_while_optional_test_is_pending() {
    assert!(asr_available_on_home(true, false, true));
    assert!(!asr_available_on_home(false, false, false));
    assert!(!asr_available_on_home(true, true, false));
}

#[test]
fn asr_configuration_rejects_an_empty_model() {
    let directory = std::env::temp_dir().join(format!("saymore-asr-validation-{}", Uuid::new_v4()));
    let store = JsonSettingsStore::at_path(directory.join("providers.json"));

    assert_eq!(
        Err(AsrConfigError::MissingModel),
        save_asr_configuration(&store, VALID_ASR_API_KEY, " ")
    );
    assert_eq!(Ok(SaymoreSettings::default()), store.load());
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn asr_configuration_normalizes_the_legacy_model_and_can_be_deleted() {
    let directory = std::env::temp_dir().join(format!("saymore-asr-round-trip-{}", Uuid::new_v4()));
    let store = JsonSettingsStore::at_path(directory.join("providers.json"));

    assert_eq!(
        Ok(()),
        save_asr_configuration(
            &store,
            &format!("  {VALID_ASR_API_KEY}  "),
            "bigmodel_async"
        )
    );
    let Ok(settings) = store.load() else {
        panic!("saved ASR settings should be readable");
    };
    assert_eq!(
        VolcengineAsrSettings {
            enabled: true,
            api_key: VALID_ASR_API_KEY.to_owned(),
            model: "volc.seedasr.sauc.duration".to_owned(),
        },
        settings.asr.volcengine
    );

    assert_eq!(Ok(()), clear_asr_configuration(&store));
    assert_eq!(Ok(SaymoreSettings::default()), store.load());
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn asr_configuration_rejects_an_unknown_volcengine_model() {
    let directory = std::env::temp_dir().join(format!("saymore-asr-model-{}", Uuid::new_v4()));
    let store = JsonSettingsStore::at_path(directory.join("providers.json"));

    assert_eq!(
        Err(AsrConfigError::UnsupportedModel),
        save_asr_configuration(&store, VALID_ASR_API_KEY, "custom-model")
    );
    assert_eq!(Ok(SaymoreSettings::default()), store.load());
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn asr_configuration_rejects_a_key_with_an_extra_printable_character() {
    let directory = std::env::temp_dir().join(format!("saymore-asr-key-format-{}", Uuid::new_v4()));
    let store = JsonSettingsStore::at_path(directory.join("providers.json"));

    assert_eq!(
        Err(AsrConfigError::InvalidApiKey),
        save_asr_configuration(&store, &format!("{VALID_ASR_API_KEY}å"), "bigmodel_async")
    );
    assert_eq!(Ok(SaymoreSettings::default()), store.load());
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn volcengine_api_key_validation_matches_the_new_console_format() {
    assert!(volcengine_api_key_is_valid(VALID_ASR_API_KEY));
    assert!(!volcengine_api_key_is_valid(&format!(
        "{VALID_ASR_API_KEY}å"
    )));
}

#[test]
fn custom_asr_configuration_round_trips_and_becomes_active() {
    let directory = std::env::temp_dir().join(format!("saymore-custom-asr-{}", Uuid::new_v4()));
    let store = JsonSettingsStore::at_path(directory.join("providers.json"));

    assert_eq!(
        Ok(()),
        save_asr_configuration(&store, VALID_ASR_API_KEY, "bigmodel_async")
    );
    assert_eq!(
        Ok(()),
        save_custom_asr_configuration(
            &store,
            " test-key ",
            " https://asr.example/v1/ ",
            " whisper-large-v3 "
        )
    );
    let Ok(settings) = store.load() else {
        panic!("saved custom ASR settings should be readable");
    };
    assert!(!settings.asr.volcengine.enabled);
    assert_eq!(
        OpenAiCompatibleAsrSettings {
            enabled: true,
            base_url: "https://asr.example/v1".to_owned(),
            api_key: "test-key".to_owned(),
            model: "whisper-large-v3".to_owned(),
        },
        settings.asr.openai_compatible
    );
    assert_eq!(
        2,
        store
            .load_catalog()
            .map(|catalog| catalog.asr_providers.len())
            .unwrap_or_default()
    );

    assert_eq!(
        Ok(()),
        save_asr_configuration(&store, VALID_ASR_API_KEY, "bigmodel_async")
    );
    let Ok(settings) = store.load() else {
        panic!("switched ASR settings should be readable");
    };
    assert!(settings.asr.volcengine.enabled);
    assert!(!settings.asr.openai_compatible.enabled);
    assert_eq!("test-key", settings.asr.openai_compatible.api_key);
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn llm_consent_is_persisted_before_connection_testing() {
    let directory = std::env::temp_dir().join(format!("saymore-llm-consent-{}", Uuid::new_v4()));
    let store = JsonSettingsStore::at_path(directory.join("providers.json"));
    let mut catalog = ProviderCatalog::default();
    catalog.save_llm_provider_config(LlmProviderPreset::SenseNova, "test-key");
    catalog.select_llm_provider(LlmProviderPreset::SenseNova);
    assert_eq!(Ok(()), store.save_catalog(&catalog));

    assert_eq!(
        Ok(()),
        persist_llm_consent(&store, LlmProviderPreset::SenseNova)
    );
    let Ok(settings) = store.load() else {
        panic!("settings with consent should be readable");
    };
    assert_eq!(
        LlmProviderPreset::SenseNova.base_url(),
        settings.llm.confirmed_base_url
    );
    assert!(!settings.llm.enabled);
    assert!(!llm_consent_required(
        &settings,
        LlmProviderPreset::SenseNova.base_url()
    ));
    let _ = fs::remove_dir_all(directory);
}
