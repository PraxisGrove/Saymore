use std::sync::Arc;

use slint::{ComponentHandle, SharedString};
use template_app::{
    LlmProviderPreset, OpenAiCompatibleAsrSettings, ProviderConfigStore, SaymoreSettings,
    SettingsStore, SettingsStoreError, SpeechRecognitionError, VolcengineAsrSettings,
};
#[cfg(test)]
use template_app::{ProviderCatalog, ProviderInstance};
#[cfg(test)]
use template_infra::AppEnvironment;
use template_infra::{
    ChatCompletionsLlmProvider, JsonSettingsStore, OpenAiCompatibleSpeechRecognizer,
    VolcengineSpeechRecognizer,
};
use uuid::Uuid;

use crate::ui::{
    AppWindow, AsrProvider as UiAsrProvider, LlmProvider as UiLlmProvider, Translations,
};

mod loaded_settings;
mod model_discovery;
mod provider_key_page;
#[cfg(test)]
mod regression_tests;

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
    }
}

fn ui_provider(provider: LlmProviderPreset) -> UiLlmProvider {
    match provider {
        LlmProviderPreset::SenseNova => UiLlmProvider::Sensenova,
        LlmProviderPreset::DeepSeek => UiLlmProvider::Deepseek,
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
    wire_asr(ui, Arc::clone(&store));
    wire_llm(ui, store);
    provider_key_page::wire(ui);
}

fn wire_asr(ui: &AppWindow, store: Arc<JsonSettingsStore>) {
    let save_ui = ui.as_weak();
    let save_store = Arc::clone(&store);
    ui.on_save_asr_config(move |provider, api_key, base_url, model| {
        let Some(ui) = save_ui.upgrade() else {
            return;
        };
        let result = match provider {
            UiAsrProvider::Volcengine => {
                save_asr_configuration(&save_store, api_key.as_str(), model.as_str())
            }
            UiAsrProvider::Custom => save_custom_asr_configuration(
                &save_store,
                api_key.as_str(),
                base_url.as_str(),
                model.as_str(),
            ),
        };
        match result {
            Ok(()) => {
                apply_status(
                    &ui,
                    true,
                    false,
                    ui.global::<Translations>().get_common_saved(),
                );
                apply_pending_test(&ui, true);
                ui.set_asr_config_dirty(false);
            }
            Err(error) => apply_status(&ui, false, true, asr_config_error_text(&ui, error)),
        }
    });

    let select_ui = ui.as_weak();
    let select_store = Arc::clone(&store);
    ui.on_select_asr_provider(move |provider| {
        let Some(ui) = select_ui.upgrade() else {
            return;
        };
        apply_asr_provider_status(&ui, &select_store, provider);
    });

    let test_ui = ui.as_weak();
    let test_store = Arc::clone(&store);
    ui.on_request_asr_test(move || {
        let Some(ui) = test_ui.upgrade() else {
            return;
        };
        begin_asr_connection_test(&ui, Arc::clone(&test_store));
    });

    let delete_ui = ui.as_weak();
    let delete_store = Arc::clone(&store);
    ui.on_delete_asr_config(move || {
        let Some(ui) = delete_ui.upgrade() else {
            return;
        };
        let result = clear_asr_configuration(&delete_store);
        match result {
            Ok(()) => {
                ui.set_asr_api_key(SharedString::new());
                apply_status(
                    &ui,
                    false,
                    false,
                    ui.global::<Translations>().get_models_not_configured(),
                );
                apply_pending_test(&ui, false);
                ui.set_asr_config_dirty(false);
            }
            Err(_) => apply_status(
                &ui,
                true,
                true,
                ui.global::<Translations>().get_common_delete_failed(),
            ),
        }
    });
}

fn begin_asr_connection_test(ui: &AppWindow, store: Arc<JsonSettingsStore>) {
    let provider = ui.get_asr_provider();
    let (api_key, base_url, model) = match provider {
        UiAsrProvider::Volcengine => (
            ui.get_asr_api_key().to_string(),
            String::new(),
            ui.get_asr_model().to_string(),
        ),
        UiAsrProvider::Custom => (
            ui.get_custom_asr_api_key().to_string(),
            ui.get_custom_asr_base_url().to_string(),
            ui.get_custom_asr_model().to_string(),
        ),
    };
    let save_result = match provider {
        UiAsrProvider::Volcengine => save_asr_configuration(&store, &api_key, &model),
        UiAsrProvider::Custom => save_custom_asr_configuration(&store, &api_key, &base_url, &model),
    };
    if let Err(error) = save_result {
        apply_status(ui, false, true, asr_config_error_text(ui, error));
        return;
    }
    ui.set_asr_testing(true);
    apply_pending_test(ui, false);
    ui.set_asr_config_error(false);
    ui.set_asr_config_status(ui.global::<Translations>().get_models_testing_connection());
    let result_ui = ui.as_weak();
    let spawn_result = std::thread::Builder::new()
        .name("saymore-test-asr".to_owned())
        .spawn(move || {
            let result = test_asr_connection(provider, api_key, base_url, model);
            if let Err(error) = &result {
                tracing::warn!(
                    target: "saymore::diagnostics",
                    event = "asr.connection_test_failed",
                    reason = %error
                );
            }
            let _ = result_ui.upgrade_in_event_loop(move |ui| {
                ui.set_asr_testing(false);
                match result {
                    Ok(()) => apply_status(
                        &ui,
                        true,
                        false,
                        ui.global::<Translations>().get_models_connected(),
                    ),
                    Err(error) => {
                        apply_status(&ui, true, true, asr_test_failure_status(&ui, &error));
                    }
                }
            });
        });
    if spawn_result.is_err() {
        ui.set_asr_testing(false);
        apply_status(
            ui,
            true,
            true,
            ui.global::<Translations>().get_models_connection_failed(),
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AsrConfigError {
    MissingApiKey,
    InvalidApiKey,
    MissingBaseUrl,
    InvalidBaseUrl,
    MissingModel,
    UnsupportedModel,
    Store,
}

fn save_asr_configuration(
    store: &JsonSettingsStore,
    api_key: &str,
    model: &str,
) -> Result<(), AsrConfigError> {
    let api_key = api_key.trim();
    let model = volcengine_model_id(model)?;
    if api_key.is_empty() {
        return Err(AsrConfigError::MissingApiKey);
    }
    if !volcengine_api_key_is_valid(api_key) {
        return Err(AsrConfigError::InvalidApiKey);
    }
    store
        .load()
        .and_then(|mut settings| {
            settings.asr.volcengine = VolcengineAsrSettings {
                enabled: true,
                api_key: api_key.to_owned(),
                model: model.to_owned(),
            };
            settings.asr.openai_compatible.enabled = false;
            store.save(&settings)
        })
        .map_err(|_| AsrConfigError::Store)
}

fn volcengine_model_id(model: &str) -> Result<&'static str, AsrConfigError> {
    match model.trim() {
        "" => Err(AsrConfigError::MissingModel),
        VOLCENGINE_ASR_1_MODEL => Ok(VOLCENGINE_ASR_1_MODEL),
        VOLCENGINE_ASR_2_MODEL | VOLCENGINE_LEGACY_MODEL => Ok(VOLCENGINE_ASR_2_MODEL),
        _ => Err(AsrConfigError::UnsupportedModel),
    }
}

fn save_custom_asr_configuration(
    store: &JsonSettingsStore,
    api_key: &str,
    base_url: &str,
    model: &str,
) -> Result<(), AsrConfigError> {
    let api_key = api_key.trim();
    let base_url = base_url.trim().trim_end_matches('/');
    let model = model.trim();
    if api_key.is_empty() {
        return Err(AsrConfigError::MissingApiKey);
    }
    if base_url.is_empty() {
        return Err(AsrConfigError::MissingBaseUrl);
    }
    if model.is_empty() {
        return Err(AsrConfigError::MissingModel);
    }
    let configuration = OpenAiCompatibleAsrSettings {
        enabled: true,
        base_url: base_url.to_owned(),
        api_key: api_key.to_owned(),
        model: model.to_owned(),
    };
    OpenAiCompatibleSpeechRecognizer::new(configuration.clone())
        .map_err(|_| AsrConfigError::InvalidBaseUrl)?;
    store
        .load()
        .and_then(|mut settings| {
            settings.asr.volcengine.enabled = false;
            settings.asr.openai_compatible = configuration;
            store.save(&settings)
        })
        .map_err(|_| AsrConfigError::Store)
}

fn volcengine_api_key_is_valid(api_key: &str) -> bool {
    api_key.len() == 36 && Uuid::parse_str(api_key).is_ok()
}

fn clear_asr_configuration(store: &JsonSettingsStore) -> Result<(), SettingsStoreError> {
    store.load_catalog().and_then(|mut catalog| {
        let active = catalog.active.asr.take();
        catalog
            .asr_providers
            .retain(|provider| Some(&provider.id) != active.as_ref());
        store.save_catalog(&catalog)
    })
}

fn apply_asr_provider_status(ui: &AppWindow, store: &JsonSettingsStore, provider: UiAsrProvider) {
    let Ok(settings) = store.load() else {
        apply_status(
            ui,
            false,
            true,
            ui.global::<Translations>()
                .get_common_configuration_load_failed(),
        );
        return;
    };
    let configured = match provider {
        UiAsrProvider::Volcengine => {
            let settings = settings.asr.volcengine;
            !settings.api_key.trim().is_empty()
                && volcengine_api_key_is_valid(settings.api_key.trim())
                && !settings.model.trim().is_empty()
        }
        UiAsrProvider::Custom => {
            let settings = settings.asr.openai_compatible;
            !settings.api_key.trim().is_empty()
                && !settings.base_url.trim().is_empty()
                && !settings.model.trim().is_empty()
        }
    };
    apply_status(
        ui,
        configured,
        false,
        if configured {
            ui.global::<Translations>().get_models_configured()
        } else {
            ui.global::<Translations>().get_models_not_configured()
        },
    );
    apply_pending_test(ui, configured);
}

fn test_asr_connection(
    provider: UiAsrProvider,
    api_key: String,
    base_url: String,
    model: String,
) -> Result<(), SpeechRecognitionError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| SpeechRecognitionError::Transport(error.to_string()))?;
    match provider {
        UiAsrProvider::Volcengine => {
            let recognizer = VolcengineSpeechRecognizer::new(VolcengineAsrSettings {
                enabled: true,
                api_key,
                model,
            })?;
            runtime.block_on(recognizer.test_connection())
        }
        UiAsrProvider::Custom => {
            let recognizer = OpenAiCompatibleSpeechRecognizer::new(OpenAiCompatibleAsrSettings {
                enabled: true,
                base_url,
                api_key,
                model,
            })?;
            runtime.block_on(recognizer.test_connection())
        }
    }
}

fn asr_test_failure_status(ui: &AppWindow, error: &SpeechRecognitionError) -> SharedString {
    let translations = ui.global::<Translations>();
    match error {
        SpeechRecognitionError::NotConfigured => translations.get_models_enter_api_key(),
        SpeechRecognitionError::Authentication => translations.get_models_test_authentication(),
        SpeechRecognitionError::Quota => translations.get_models_test_quota(),
        SpeechRecognitionError::Transport(_) => translations.get_models_test_transport(),
        SpeechRecognitionError::Protocol(_) => translations.get_models_test_protocol(),
        SpeechRecognitionError::Timeout => translations.get_models_test_timeout(),
    }
}

fn asr_config_error_text(ui: &AppWindow, error: AsrConfigError) -> SharedString {
    let translations = ui.global::<Translations>();
    match error {
        AsrConfigError::MissingApiKey => translations.get_models_enter_api_key(),
        AsrConfigError::InvalidApiKey => translations.get_models_invalid_api_key(),
        AsrConfigError::MissingBaseUrl => translations.get_models_enter_service_url(),
        AsrConfigError::InvalidBaseUrl => translations.get_models_invalid_service_url(),
        AsrConfigError::MissingModel => translations.get_models_enter_model_name(),
        AsrConfigError::UnsupportedModel => translations.get_models_unsupported_volcengine_model(),
        AsrConfigError::Store => translations.get_common_save_failed(),
    }
}

fn wire_llm(ui: &AppWindow, store: Arc<JsonSettingsStore>) {
    wire_llm_config_actions(ui, Arc::clone(&store));
    wire_llm_enablement(ui, store);
}

fn wire_llm_config_actions(ui: &AppWindow, store: Arc<JsonSettingsStore>) {
    let save_ui = ui.as_weak();
    let save_store = Arc::clone(&store);
    ui.on_save_llm_config(move |provider, api_key, model| {
        let Some(ui) = save_ui.upgrade() else {
            return;
        };
        let provider = provider_preset(provider);
        if api_key.trim().is_empty() {
            ui.set_llm_config_status(ui.global::<Translations>().get_models_enter_api_key());
            return;
        }
        if model.trim().is_empty() {
            ui.set_llm_config_status(ui.global::<Translations>().get_models_choose_model());
            return;
        }
        let result = save_store.load_catalog().and_then(|mut catalog| {
            catalog.save_llm_provider_model_config(provider, api_key.as_str(), model.as_str());
            save_store.save_catalog(&catalog)
        });
        if result.is_ok() {
            apply_loaded_settings(&ui, &save_store);
            ui.set_llm_config_status(ui.global::<Translations>().get_models_saved_enable_test());
            ui.set_llm_config_dirty(false);
        } else {
            ui.set_llm_config_status(ui.global::<Translations>().get_common_save_failed());
        }
    });

    let select_ui = ui.as_weak();
    let select_store = Arc::clone(&store);
    ui.on_select_llm_provider(move |provider| {
        let Some(ui) = select_ui.upgrade() else {
            return;
        };
        let provider = provider_preset(provider);
        let result = select_store.load_catalog().and_then(|mut catalog| {
            catalog.select_llm_provider(provider);
            select_store.save_catalog(&catalog)
        });
        if result.is_ok() {
            apply_loaded_settings(&ui, &select_store);
            ui.set_llm_provider_target(SharedString::from(provider.base_url()));
        } else {
            ui.set_llm_config_status(ui.global::<Translations>().get_models_switch_failed());
        }
    });
}

fn wire_llm_enablement(ui: &AppWindow, store: Arc<JsonSettingsStore>) {
    let prepare_ui = ui.as_weak();
    let prepare_store = Arc::clone(&store);
    ui.on_request_llm_enable(move || {
        let Some(ui) = prepare_ui.upgrade() else {
            return;
        };
        let provider = provider_preset(ui.get_llm_provider());
        let Ok(catalog) = prepare_store.load_catalog() else {
            ui.set_llm_config_status(
                ui.global::<Translations>()
                    .get_common_configuration_load_failed(),
            );
            return;
        };
        if catalog
            .llm_provider_api_key(provider)
            .is_none_or(str::is_empty)
        {
            ui.set_llm_config_status(ui.global::<Translations>().get_models_save_api_key_first());
            return;
        }
        let base_url = provider.base_url().to_owned();
        let local = provider_is_local(&base_url);
        ui.set_llm_provider_target(SharedString::from(&base_url));
        ui.set_llm_provider_local(local);
        let Ok(settings) = prepare_store.load() else {
            ui.set_llm_config_status(
                ui.global::<Translations>()
                    .get_common_configuration_load_failed(),
            );
            return;
        };
        if llm_consent_required(&settings, &base_url) {
            ui.set_llm_confirmation_visible(true);
        } else {
            start_llm_test(&ui, Arc::clone(&prepare_store), provider);
        }
    });

    let llm_ui = ui.as_weak();
    ui.on_set_llm_enabled(move |enabled, expected_base_url| {
        let Some(ui) = llm_ui.upgrade() else {
            return;
        };
        if !enabled {
            let result = store.load().and_then(|mut settings| {
                settings.llm.enabled = false;
                store.save(&settings)
            });
            ui.set_llm_enabled(result.is_err());
            let translations = ui.global::<Translations>();
            ui.set_llm_config_status(if result.is_ok() {
                translations.get_models_not_enabled()
            } else {
                translations.get_common_save_failed()
            });
            return;
        }
        let provider = provider_preset(ui.get_llm_provider());
        if provider.base_url() != expected_base_url.trim() {
            ui.set_llm_config_status(ui.global::<Translations>().get_models_provider_changed());
            return;
        }
        if persist_llm_consent(&store, provider).is_err() {
            ui.set_llm_config_status(
                ui.global::<Translations>()
                    .get_models_authorization_save_failed(),
            );
            return;
        }
        start_llm_test(&ui, Arc::clone(&store), provider);
    });
}

fn llm_consent_required(settings: &SaymoreSettings, expected_base_url: &str) -> bool {
    settings.llm.confirmed_base_url.trim() != expected_base_url.trim()
}

fn persist_llm_consent(
    store: &JsonSettingsStore,
    provider: LlmProviderPreset,
) -> Result<(), SettingsStoreError> {
    store.load().and_then(|mut settings| {
        settings.llm.enabled = false;
        settings.llm.confirmed_base_url = provider.base_url().to_owned();
        store.save(&settings)
    })
}

fn start_llm_test(ui: &AppWindow, store: Arc<JsonSettingsStore>, provider: LlmProviderPreset) {
    ui.set_llm_enabled(false);
    ui.set_llm_config_status(ui.global::<Translations>().get_models_testing_connection());
    let test_ui = ui.as_weak();
    let spawn_result = std::thread::Builder::new()
        .name("saymore-test-llm".to_owned())
        .spawn(move || {
            let result = test_and_enable_llm(&store, provider);
            let event_store = Arc::clone(&store);
            let _ = test_ui.upgrade_in_event_loop(move |ui| {
                apply_loaded_settings(&ui, &event_store);
                if result.is_err() {
                    ui.set_llm_config_status(ui.global::<Translations>().get_models_test_failed());
                }
            });
        });
    if spawn_result.is_err() {
        ui.set_llm_config_status(ui.global::<Translations>().get_models_test_failed());
    }
}

fn test_and_enable_llm(
    store: &JsonSettingsStore,
    provider_preset: LlmProviderPreset,
) -> Result<(), String> {
    let mut catalog = store.load_catalog().map_err(|error| error.to_string())?;
    let api_key = catalog
        .llm_provider_api_key(provider_preset)
        .ok_or_else(|| "LLM API Key is missing".to_owned())?
        .to_owned();
    let provider_settings = provider_preset.settings(&api_key);
    catalog.select_llm_provider(provider_preset);
    let expected_provider_id = catalog
        .active
        .llm
        .clone()
        .ok_or_else(|| "LLM provider selection failed".to_owned())?;
    store
        .save_catalog(&catalog)
        .map_err(|error| error.to_string())?;
    let provider = ChatCompletionsLlmProvider::new(provider_settings.clone())
        .map_err(|error| error.to_string())?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    runtime
        .block_on(provider.test_connection())
        .map_err(|error| error.to_string())?;
    if !store
        .enable_llm_provider_if_unchanged(
            provider_preset,
            &expected_provider_id,
            &provider_settings.api_key,
        )
        .map_err(|error| error.to_string())?
    {
        return Err("LLM provider changed during connection test".to_owned());
    }
    Ok(())
}

fn provider_is_local(base_url: &str) -> bool {
    let authority = base_url
        .split_once("://")
        .map_or(base_url, |(_, remainder)| remainder)
        .split('/')
        .next()
        .unwrap_or_default()
        .rsplit('@')
        .next()
        .unwrap_or_default();
    let host = if let Some(bracketed) = authority.strip_prefix('[') {
        bracketed.split(']').next().unwrap_or_default()
    } else {
        authority.split(':').next().unwrap_or_default()
    };
    matches!(
        host.to_ascii_lowercase().as_str(),
        "localhost" | "127.0.0.1" | "::1"
    )
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
            provider_key_page::VOLCENGINE_KEY_PAGE,
            provider_key_page::url(0, UiLlmProvider::Sensenova)
        );
        assert_eq!(
            provider_key_page::SENSENOVA_KEY_PAGE,
            provider_key_page::url(1, UiLlmProvider::Sensenova)
        );
        assert_eq!(
            provider_key_page::DEEPSEEK_KEY_PAGE,
            provider_key_page::url(1, UiLlmProvider::Deepseek)
        );
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
