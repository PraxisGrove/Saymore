use std::sync::Arc;

use slint::{ComponentHandle, SharedString};
use template_app::{
    ChatCompletionsLlmSettings, LlmProviderPreset, ProviderConfigStore, SaymoreSettings,
    SettingsStore, SettingsStoreError,
};
use template_infra::{ChatCompletionsLlmProvider, JsonSettingsStore};

use crate::ui::{AppWindow, Translations};

use super::{apply_loaded_settings, provider_preset};

pub(super) fn wire(ui: &AppWindow, store: Arc<JsonSettingsStore>) {
    wire_enable_request(ui, Arc::clone(&store));
    wire_enable_confirmation(ui, store);
}

fn wire_enable_request(ui: &AppWindow, store: Arc<JsonSettingsStore>) {
    let prepare_ui = ui.as_weak();
    ui.on_request_llm_enable(move || {
        let Some(ui) = prepare_ui.upgrade() else {
            return;
        };
        let provider = provider_preset(ui.get_llm_provider());
        let Ok(catalog) = store.load_catalog() else {
            ui.set_llm_config_status(
                ui.global::<Translations>()
                    .get_common_configuration_load_failed(),
            );
            return;
        };
        let Some(provider_settings) = catalog.llm_provider_settings(provider) else {
            ui.set_llm_config_status(ui.global::<Translations>().get_models_save_api_key_first());
            return;
        };
        if !llm_configuration_ready(provider, &provider_settings) {
            ui.set_llm_config_status(ui.global::<Translations>().get_models_save_api_key_first());
            return;
        }
        let base_url = provider_settings.base_url;
        let local = provider_is_local(&base_url);
        ui.set_llm_provider_target(SharedString::from(&base_url));
        ui.set_llm_provider_local(local);
        let Ok(settings) = store.load() else {
            ui.set_llm_config_status(
                ui.global::<Translations>()
                    .get_common_configuration_load_failed(),
            );
            return;
        };
        if llm_consent_required(&settings, &base_url) {
            ui.set_llm_confirmation_visible(true);
        } else {
            start_llm_test(&ui, Arc::clone(&store), provider);
        }
    });
}

fn wire_enable_confirmation(ui: &AppWindow, store: Arc<JsonSettingsStore>) {
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
        let Ok(catalog) = store.load_catalog() else {
            ui.set_llm_config_status(
                ui.global::<Translations>()
                    .get_common_configuration_load_failed(),
            );
            return;
        };
        let Some(settings) = catalog.llm_provider_settings(provider) else {
            ui.set_llm_config_status(ui.global::<Translations>().get_models_provider_changed());
            return;
        };
        if settings.base_url != expected_base_url.trim() {
            ui.set_llm_config_status(ui.global::<Translations>().get_models_provider_changed());
            return;
        }
        if persist_llm_consent(&store, &settings.base_url).is_err() {
            ui.set_llm_config_status(
                ui.global::<Translations>()
                    .get_models_authorization_save_failed(),
            );
            return;
        }
        start_llm_test(&ui, Arc::clone(&store), provider);
    });
}

pub(super) fn llm_consent_required(settings: &SaymoreSettings, expected_base_url: &str) -> bool {
    settings.llm.confirmed_base_url.trim() != expected_base_url.trim()
}

pub(super) fn persist_llm_consent(
    store: &JsonSettingsStore,
    base_url: &str,
) -> Result<(), SettingsStoreError> {
    store.load().and_then(|mut settings| {
        settings.llm.enabled = false;
        settings.llm.confirmed_base_url = base_url.to_owned();
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
            match &result {
                Ok(()) => tracing::info!(
                    target: "saymore::diagnostics",
                    event = "llm.enabled"
                ),
                Err(error) => tracing::warn!(
                    target: "saymore::diagnostics",
                    event = "llm.enable_failed",
                    reason = %error
                ),
            }
            let event_store = Arc::clone(&store);
            let _ = test_ui.upgrade_in_event_loop(move |ui| {
                apply_loaded_settings(&ui, &event_store);
                if result.is_err() {
                    ui.set_llm_config_status(ui.global::<Translations>().get_models_test_failed());
                }
            });
        });
    if spawn_result.is_err() {
        tracing::warn!(
            target: "saymore::diagnostics",
            event = "llm.enable_worker_start_failed"
        );
        ui.set_llm_config_status(ui.global::<Translations>().get_models_test_failed());
    }
}

pub(super) fn test_and_enable_llm(
    store: &JsonSettingsStore,
    provider_preset: LlmProviderPreset,
) -> Result<(), String> {
    let mut catalog = store.load_catalog().map_err(|error| error.to_string())?;
    let provider_settings = catalog
        .llm_provider_settings(provider_preset)
        .filter(|settings| llm_configuration_ready(provider_preset, settings))
        .ok_or_else(|| "LLM configuration is incomplete".to_owned())?;
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
            &expected_provider_id,
            &provider_settings.base_url,
            &provider_settings.api_key,
        )
        .map_err(|error| error.to_string())?
    {
        return Err("LLM provider changed during connection test".to_owned());
    }
    Ok(())
}

pub(super) fn llm_configuration_ready(
    provider: LlmProviderPreset,
    settings: &ChatCompletionsLlmSettings,
) -> bool {
    !settings.base_url.trim().is_empty()
        && !settings.model.trim().is_empty()
        && (provider == LlmProviderPreset::Custom || !settings.api_key.trim().is_empty())
}

pub(super) fn provider_is_local(base_url: &str) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_provider_can_omit_an_api_key() {
        let settings = ChatCompletionsLlmSettings {
            base_url: "http://localhost:11434/v1".to_owned(),
            api_key: String::new(),
            model: "qwen3:8b".to_owned(),
            custom_headers: Default::default(),
        };

        assert!(llm_configuration_ready(
            LlmProviderPreset::Custom,
            &settings
        ));
        assert!(!llm_configuration_ready(
            LlmProviderPreset::DeepSeek,
            &settings
        ));
    }
}
