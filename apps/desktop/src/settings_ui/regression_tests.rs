use std::fs;

use template_app::{
    LlmProviderPreset, ProviderCatalog, ProviderConfigStore, SaymoreSettings, SettingsStore,
    SpeechRecognitionError, VolcengineAsrSettings,
};
use template_infra::JsonSettingsStore;
use uuid::Uuid;

use super::{
    AsrConfigError, asr_test_failure_status, clear_asr_configuration, llm_consent_required,
    persist_llm_consent, save_asr_configuration,
};

#[test]
fn asr_configuration_rejects_an_empty_model() {
    let directory = std::env::temp_dir().join(format!("saymore-asr-validation-{}", Uuid::new_v4()));
    let store = JsonSettingsStore::at_path(directory.join("providers.json"));

    assert_eq!(
        Err(AsrConfigError::MissingModel),
        save_asr_configuration(&store, "test-key", " ")
    );
    assert_eq!(Ok(SaymoreSettings::default()), store.load());
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn asr_configuration_round_trips_a_custom_model_and_can_be_deleted() {
    let directory = std::env::temp_dir().join(format!("saymore-asr-round-trip-{}", Uuid::new_v4()));
    let store = JsonSettingsStore::at_path(directory.join("providers.json"));

    assert_eq!(
        Ok(()),
        save_asr_configuration(&store, "  test-key  ", "  custom-model  ")
    );
    let Ok(settings) = store.load() else {
        panic!("saved ASR settings should be readable");
    };
    assert_eq!(
        VolcengineAsrSettings {
            enabled: true,
            api_key: "test-key".to_owned(),
            model: "custom-model".to_owned(),
        },
        settings.asr.volcengine
    );

    assert_eq!(Ok(()), clear_asr_configuration(&store));
    assert_eq!(Ok(SaymoreSettings::default()), store.load());
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn asr_test_errors_are_reported_by_category() {
    assert_eq!(
        "API Key 无效，请检查后重试",
        asr_test_failure_status(&SpeechRecognitionError::Authentication)
    );
    assert_eq!(
        "无法连接语音服务，请检查网络后重试",
        asr_test_failure_status(&SpeechRecognitionError::Transport("offline".to_owned()))
    );
    assert_eq!(
        "当前语音服务额度不可用",
        asr_test_failure_status(&SpeechRecognitionError::Quota)
    );
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
