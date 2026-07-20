use std::sync::Arc;

use slint::{ComponentHandle, SharedString};
use template_app::LlmProviderPreset;
#[cfg(test)]
use template_app::{ProviderCatalog, ProviderConfigStore, ProviderInstance};
#[cfg(test)]
use template_infra::AppEnvironment;
use template_infra::JsonSettingsStore;

use crate::ui::{AppWindow, LlmProvider as UiLlmProvider, Translations};

mod asr_configuration;
mod llm_configuration;
mod llm_enablement;
mod loaded_settings;
mod model_discovery;
mod provider_key_page;
#[cfg(test)]
mod regression_tests;

#[cfg(test)]
use asr_configuration::{
    AsrConfigError, clear_asr_configuration, save_asr_configuration, save_custom_asr_configuration,
};
use asr_configuration::{volcengine_api_key_is_valid, volcengine_model_id};
use llm_enablement::{llm_configuration_ready, provider_is_local};
#[cfg(test)]
use llm_enablement::{llm_consent_required, persist_llm_consent, test_and_enable_llm};
use loaded_settings::apply_loaded_settings;

const VOLCENGINE_ASR_1_MODEL: &str = "volc.bigasr.sauc.duration";
const VOLCENGINE_ASR_2_MODEL: &str = "volc.seedasr.sauc.duration";
const VOLCENGINE_LEGACY_MODEL: &str = "bigmodel_async";
const VOLCENGINE_MODELS: [&str; 2] = [VOLCENGINE_ASR_2_MODEL, VOLCENGINE_ASR_1_MODEL];
#[cfg(test)]
const CHAT_COMPLETIONS_TYPE: &str = "openai_compatible";

fn provider_preset(provider: UiLlmProvider) -> LlmProviderPreset {
    match provider {
        UiLlmProvider::Sensenova => LlmProviderPreset::SenseNova,
        UiLlmProvider::Deepseek => LlmProviderPreset::DeepSeek,
        UiLlmProvider::Custom => LlmProviderPreset::Custom,
    }
}

fn ui_provider(provider: LlmProviderPreset) -> UiLlmProvider {
    match provider {
        LlmProviderPreset::SenseNova => UiLlmProvider::Sensenova,
        LlmProviderPreset::DeepSeek => UiLlmProvider::Deepseek,
        LlmProviderPreset::Custom => UiLlmProvider::Custom,
    }
}

pub fn wire(ui: &AppWindow, store: Arc<JsonSettingsStore>) {
    apply_loaded_settings(ui, &store);
    let refresh_ui = ui.as_weak();
    let refresh_store = Arc::clone(&store);
    ui.on_refresh_localized_state(move || {
        if let Some(ui) = refresh_ui.upgrade() {
            apply_loaded_settings(&ui, &refresh_store);
        }
    });
    model_discovery::wire(ui);
    asr_configuration::wire(ui, Arc::clone(&store));
    wire_llm(ui, store);
    provider_key_page::wire(ui);
}

fn wire_llm(ui: &AppWindow, store: Arc<JsonSettingsStore>) {
    llm_configuration::wire(ui, Arc::clone(&store));
    llm_enablement::wire(ui, store);
}

fn apply_status(ui: &AppWindow, configured: bool, error: bool, status: impl Into<SharedString>) {
    ui.set_asr_configured(configured);
    ui.set_asr_config_error(error);
    ui.set_asr_config_status(status.into());
    ui.set_asr_home_available(asr_available_on_home(
        configured,
        error,
        ui.get_asr_pending_test(),
    ));
}

fn apply_pending_test(ui: &AppWindow, pending_test: bool) {
    ui.set_asr_pending_test(pending_test);
    ui.set_asr_home_available(asr_available_on_home(
        ui.get_asr_configured(),
        ui.get_asr_config_error(),
        pending_test,
    ));
}

fn asr_available_on_home(configured: bool, error: bool, _pending_test: bool) -> bool {
    configured && !error
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn mark_asr_runtime_healthy(ui: &AppWindow) {
    apply_pending_test(ui, false);
    apply_status(
        ui,
        true,
        false,
        ui.global::<Translations>().get_models_connected(),
    );
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use uuid::Uuid;

    #[test]
    fn sensenova_configuration_uses_the_supported_endpoint_and_model() {
        let settings = LlmProviderPreset::SenseNova.settings("test-key");

        assert_eq!(LlmProviderPreset::SenseNova.base_url(), settings.base_url);
        assert_eq!(LlmProviderPreset::SenseNova.model(), settings.model);
        assert_eq!("test-key", settings.api_key);
        assert!(settings.custom_headers.is_empty());
    }

    #[test]
    fn deepseek_configuration_uses_the_official_chat_completions_api() {
        let settings = LlmProviderPreset::DeepSeek.settings("deepseek-key");

        assert_eq!("https://api.deepseek.com", settings.base_url);
        assert_eq!("deepseek-v4-flash", settings.model);
        assert_eq!("deepseek-key", settings.api_key);
        assert!(settings.custom_headers.is_empty());
    }

    #[test]
    fn routes_key_page_actions_to_the_selected_provider() {
        assert_eq!(
            Some(provider_key_page::VOLCENGINE_KEY_PAGE),
            provider_key_page::url(0, UiLlmProvider::Sensenova)
        );
        assert_eq!(
            Some(provider_key_page::SENSENOVA_KEY_PAGE),
            provider_key_page::url(1, UiLlmProvider::Sensenova)
        );
        assert_eq!(
            Some(provider_key_page::DEEPSEEK_KEY_PAGE),
            provider_key_page::url(1, UiLlmProvider::Deepseek)
        );
        assert_eq!(None, provider_key_page::url(1, UiLlmProvider::Custom));
    }

    #[test]
    fn persists_both_provider_keys_and_selects_deepseek() {
        let directory =
            std::env::temp_dir().join(format!("saymore-provider-switch-{}", Uuid::new_v4()));
        let store = JsonSettingsStore::at_path(directory.join("providers.json"));
        let mut catalog = ProviderCatalog::default();

        catalog.save_llm_provider_config(LlmProviderPreset::SenseNova, "sense-key");
        catalog.save_llm_provider_config(LlmProviderPreset::DeepSeek, "deepseek-key");
        catalog.select_llm_provider(LlmProviderPreset::DeepSeek);
        assert_eq!(Ok(()), store.save_catalog(&catalog));
        let Ok(catalog) = store.load_catalog() else {
            panic!("saved provider catalog should be readable");
        };

        assert_eq!(
            Some("sense-key"),
            catalog.llm_provider_api_key(LlmProviderPreset::SenseNova)
        );
        assert_eq!(
            Some("deepseek-key"),
            catalog.llm_provider_api_key(LlmProviderPreset::DeepSeek)
        );
        assert_eq!(Some("deepseek"), catalog.active.llm.as_deref());
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn adopts_an_active_legacy_sensenova_instance_without_losing_selection() {
        let mut catalog = ProviderCatalog {
            active: template_app::ActiveProviders {
                asr: None,
                llm: Some("legacy-id".to_owned()),
            },
            asr_providers: Vec::new(),
            llm_providers: vec![ProviderInstance {
                id: "legacy-id".to_owned(),
                name: "OpenAI-compatible".to_owned(),
                provider_type: CHAT_COMPLETIONS_TYPE.to_owned(),
                config: serde_json::json!({
                    "base_url": LlmProviderPreset::SenseNova.base_url(),
                    "api_key": "legacy-key",
                    "model": LlmProviderPreset::SenseNova.model(),
                }),
                data_consent: None,
            }],
        };

        catalog.save_llm_provider_config(LlmProviderPreset::SenseNova, "legacy-key");

        assert_eq!(Some("sensenova"), catalog.active.llm.as_deref());
        assert_eq!(
            Some("legacy-key"),
            catalog.llm_provider_api_key(LlmProviderPreset::SenseNova)
        );
    }

    #[test]
    #[ignore = "uses and enables the current user's live SenseNova configuration"]
    fn current_user_sensenova_configuration_can_be_enabled() {
        let Ok(store) = JsonSettingsStore::for_current_user(AppEnvironment::Production) else {
            panic!("current user settings should be available");
        };

        let result = test_and_enable_llm(&store, LlmProviderPreset::SenseNova);

        assert!(result.is_ok(), "SenseNova enablement failed: {result:?}");
    }
}
