use std::sync::Arc;

use slint::{ComponentHandle, SharedString};
use template_app::{LlmProviderPreset, ProviderConfigStore};
#[cfg(test)]
use template_app::{ProviderCatalog, ProviderInstance};
#[cfg(test)]
use template_infra::AppEnvironment;
use template_infra::JsonSettingsStore;

use crate::ui::{
    AppWindow, LlmProvider as UiLlmProvider, SystemSpeechStatus as UiSystemSpeechStatus,
    Translations,
};

#[cfg(target_os = "macos")]
use template_infra::{
    MacOsSpeechAuthorization, macos_speech_capability, open_speech_recognition_privacy_settings,
    request_macos_speech_authorization,
};

mod asr_configuration;
mod asr_provider_cards;
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
    asr_provider_cards::apply(ui);
    wire_macos_speech(ui, Arc::clone(&store));
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

#[cfg(target_os = "macos")]
fn wire_macos_speech(ui: &AppWindow, store: Arc<JsonSettingsStore>) {
    let activation_ui = ui.as_weak();
    let activation_store = Arc::clone(&store);
    ui.on_activate_macos_speech(move || {
        let capability = macos_speech_capability();
        match capability.authorization {
            MacOsSpeechAuthorization::Authorized if capability.available => {
                select_macos_speech(&activation_ui, &activation_store);
            }
            MacOsSpeechAuthorization::NotDetermined => {
                let result_ui = activation_ui.clone();
                let result_store = Arc::clone(&activation_store);
                request_macos_speech_authorization(Arc::new(move |_| {
                    let result_store = Arc::clone(&result_store);
                    let _ = result_ui.upgrade_in_event_loop(move |ui| {
                        let capability = macos_speech_capability();
                        if capability.authorization == MacOsSpeechAuthorization::Authorized
                            && capability.available
                        {
                            select_macos_speech(&ui.as_weak(), &result_store);
                        } else {
                            apply_loaded_settings(&ui, &result_store);
                        }
                    });
                }));
            }
            MacOsSpeechAuthorization::Denied | MacOsSpeechAuthorization::Restricted => {
                if let Err(error) = open_speech_recognition_privacy_settings() {
                    tracing::warn!(
                        target: "saymore::diagnostics",
                        event = "macos_speech.settings_open_failed",
                        reason = %error
                    );
                }
            }
            MacOsSpeechAuthorization::Authorized => {
                if let Some(ui) = activation_ui.upgrade() {
                    apply_loaded_settings(&ui, &activation_store);
                }
            }
        }
    });

    let refresh_ui = ui.as_weak();
    ui.on_refresh_macos_speech(move || {
        if let Some(ui) = refresh_ui.upgrade() {
            let selected = ui.get_macos_speech_selected();
            let ready = apply_macos_speech_state(&ui, selected);
            if selected {
                let translations = ui.global::<Translations>();
                apply_status(
                    &ui,
                    ready,
                    false,
                    if ready {
                        translations.get_models_configured()
                    } else {
                        translations.get_models_not_configured()
                    },
                );
            }
        }
    });
}

#[cfg(not(target_os = "macos"))]
fn wire_macos_speech(_ui: &AppWindow, _store: Arc<JsonSettingsStore>) {}

#[cfg(target_os = "macos")]
fn select_macos_speech(ui: &slint::Weak<AppWindow>, store: &JsonSettingsStore) {
    let result = store.load_catalog().and_then(|mut catalog| {
        catalog.select_macos_speech_provider();
        store.save_catalog(&catalog)
    });
    if let Some(ui) = ui.upgrade() {
        match result {
            Ok(()) => apply_loaded_settings(&ui, store),
            Err(error) => {
                tracing::warn!(
                    target: "saymore::diagnostics",
                    event = "macos_speech.selection_save_failed",
                    reason = %error
                );
                apply_status(
                    &ui,
                    false,
                    true,
                    ui.global::<Translations>()
                        .get_common_configuration_load_failed(),
                );
            }
        }
    }
}

pub(super) fn apply_macos_speech_state(ui: &AppWindow, selected: bool) -> bool {
    #[cfg(target_os = "macos")]
    let status = {
        let capability = macos_speech_capability();
        match capability.authorization {
            MacOsSpeechAuthorization::NotDetermined => UiSystemSpeechStatus::PermissionRequired,
            MacOsSpeechAuthorization::Denied => UiSystemSpeechStatus::PermissionDenied,
            MacOsSpeechAuthorization::Restricted => UiSystemSpeechStatus::Restricted,
            MacOsSpeechAuthorization::Authorized if capability.available => {
                UiSystemSpeechStatus::Ready
            }
            MacOsSpeechAuthorization::Authorized => UiSystemSpeechStatus::Unavailable,
        }
    };
    #[cfg(not(target_os = "macos"))]
    let status = UiSystemSpeechStatus::Unavailable;

    ui.set_macos_speech_selected(selected);
    ui.set_macos_speech_status(status);
    status == UiSystemSpeechStatus::Ready
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
