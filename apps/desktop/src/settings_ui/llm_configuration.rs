use std::sync::Arc;

use slint::ComponentHandle;
use template_app::{
    ChatCompletionsLlmSettings, LlmProviderPreset, ProviderConfigStore, SettingsStore,
    SettingsStoreError,
};
use template_infra::{ChatCompletionsLlmProvider, JsonSettingsStore};

use crate::ui::{AppWindow, LlmProvider as UiLlmProvider, Translations};

use super::llm_enablement::{llm_consent_required, provider_is_local};
use super::{apply_loaded_settings, provider_preset};

#[derive(Clone)]
struct LlmCandidate {
    provider: LlmProviderPreset,
    settings: ChatCompletionsLlmSettings,
}

#[derive(Debug)]
enum LlmSaveError {
    Invalid,
    Connection,
    Store,
    Changed,
}

pub(super) fn wire(ui: &AppWindow, store: Arc<JsonSettingsStore>) {
    let save_ui = ui.as_weak();
    let save_store = Arc::clone(&store);
    ui.on_save_llm_config(move |provider, api_key, base_url, model| {
        let Some(ui) = save_ui.upgrade() else {
            return;
        };
        let candidate = candidate(provider, &api_key, &base_url, &model);
        begin_activation(&ui, Arc::clone(&save_store), candidate);
    });

    let confirm_ui = ui.as_weak();
    let confirm_store = Arc::clone(&store);
    ui.on_confirm_llm_config(
        move |provider, api_key, base_url, model, expected_base_url| {
            let Some(ui) = confirm_ui.upgrade() else {
                return;
            };
            let candidate = candidate(provider, &api_key, &base_url, &model);
            let candidate = match candidate {
                Ok(candidate) if candidate.settings.base_url == expected_base_url.trim() => {
                    candidate
                }
                Ok(_) => {
                    ui.set_llm_config_status(
                        ui.global::<Translations>().get_models_provider_changed(),
                    );
                    return;
                }
                Err(_) => {
                    apply_validation_error(&ui);
                    return;
                }
            };
            start_activation(&ui, Arc::clone(&confirm_store), candidate);
        },
    );

    let select_ui = ui.as_weak();
    ui.on_select_llm_provider(move |provider| {
        let Some(ui) = select_ui.upgrade() else {
            return;
        };
        let provider = provider_preset(provider);
        let result = store.load_catalog().and_then(|mut catalog| {
            catalog.select_llm_provider(provider);
            store.save_catalog(&catalog)
        });
        if result.is_ok() {
            apply_loaded_settings(&ui, &store);
            ui.set_llm_provider_target(provider_base_url(&ui, provider).into());
        } else {
            ui.set_llm_config_status(ui.global::<Translations>().get_models_switch_failed());
        }
    });
}

fn candidate(
    provider: UiLlmProvider,
    api_key: &str,
    base_url: &str,
    model: &str,
) -> Result<LlmCandidate, LlmSaveError> {
    let provider = provider_preset(provider);
    let api_key = api_key.trim();
    if provider != LlmProviderPreset::Custom && api_key.is_empty() {
        return Err(LlmSaveError::Invalid);
    }
    let base_url = if provider == LlmProviderPreset::Custom {
        base_url.trim().trim_end_matches('/').to_owned()
    } else {
        provider.base_url().to_owned()
    };
    let model = model.trim();
    if base_url.is_empty() || model.is_empty() {
        return Err(LlmSaveError::Invalid);
    }
    let settings = ChatCompletionsLlmSettings {
        base_url,
        api_key: api_key.to_owned(),
        model: model.to_owned(),
        custom_headers: Default::default(),
    };
    ChatCompletionsLlmProvider::new(settings.clone()).map_err(|_| LlmSaveError::Invalid)?;
    Ok(LlmCandidate { provider, settings })
}

fn begin_activation(
    ui: &AppWindow,
    store: Arc<JsonSettingsStore>,
    candidate: Result<LlmCandidate, LlmSaveError>,
) {
    let candidate = match candidate {
        Ok(candidate) => candidate,
        Err(_) => {
            apply_validation_error(ui);
            return;
        }
    };
    let base_url = candidate.settings.base_url.as_str();
    ui.set_llm_provider_target(base_url.into());
    ui.set_llm_provider_local(provider_is_local(base_url));
    let Ok(settings) = store.load() else {
        ui.set_llm_config_status(
            ui.global::<Translations>()
                .get_common_configuration_load_failed(),
        );
        return;
    };
    if llm_consent_required(&settings, base_url) {
        ui.set_llm_confirmation_for_draft(true);
        ui.set_llm_confirmation_visible(true);
        return;
    }
    start_activation(ui, store, candidate);
}

fn start_activation(ui: &AppWindow, store: Arc<JsonSettingsStore>, candidate: LlmCandidate) {
    ui.set_llm_testing(true);
    ui.set_llm_draft_error(false);
    ui.set_llm_config_status(ui.global::<Translations>().get_models_testing_connection());
    let result_ui = ui.as_weak();
    let spawn_result = std::thread::Builder::new()
        .name("saymore-test-llm-config".to_owned())
        .spawn(move || {
            let result = test_save_and_enable(&store, &candidate, test_connection);
            if let Err(error) = &result {
                tracing::warn!(
                    target: "saymore::diagnostics",
                    event = "llm.configuration_test_failed",
                    reason = ?error
                );
            }
            let _ = result_ui.upgrade_in_event_loop(move |ui| {
                ui.set_llm_testing(false);
                match result {
                    Ok(()) => {
                        tracing::info!(
                            target: "saymore::diagnostics",
                            event = "llm.configuration_saved"
                        );
                        apply_loaded_settings(&ui, &store);
                        ui.set_llm_config_status(ui.global::<Translations>().get_models_enabled());
                    }
                    Err(_) => {
                        ui.set_llm_draft_error(true);
                        ui.set_llm_config_status(
                            ui.global::<Translations>().get_models_connection_failed(),
                        );
                    }
                }
            });
        });
    if spawn_result.is_err() {
        tracing::warn!(
            target: "saymore::diagnostics",
            event = "llm.configuration_test_worker_start_failed"
        );
        ui.set_llm_testing(false);
        ui.set_llm_draft_error(true);
        ui.set_llm_config_status(ui.global::<Translations>().get_models_connection_failed());
    }
}

fn test_save_and_enable(
    store: &JsonSettingsStore,
    candidate: &LlmCandidate,
    test: impl FnOnce(&ChatCompletionsLlmSettings) -> Result<(), String>,
) -> Result<(), LlmSaveError> {
    test(&candidate.settings).map_err(|_| LlmSaveError::Connection)?;
    save_candidate(store, candidate).map_err(|_| LlmSaveError::Store)?;
    let enabled = store
        .enable_llm_provider_if_unchanged(
            candidate.provider.id(),
            &candidate.settings.base_url,
            &candidate.settings.api_key,
        )
        .map_err(|_| LlmSaveError::Store)?;
    enabled.then_some(()).ok_or(LlmSaveError::Changed)
}

fn test_connection(settings: &ChatCompletionsLlmSettings) -> Result<(), String> {
    let provider =
        ChatCompletionsLlmProvider::new(settings.clone()).map_err(|error| error.to_string())?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    runtime
        .block_on(provider.test_connection())
        .map_err(|error| error.to_string())
}

fn save_candidate(
    store: &JsonSettingsStore,
    candidate: &LlmCandidate,
) -> Result<(), SettingsStoreError> {
    store.load_catalog().and_then(|mut catalog| {
        if candidate.provider == LlmProviderPreset::Custom {
            catalog.save_custom_llm_provider_config(
                &candidate.settings.base_url,
                &candidate.settings.api_key,
                &candidate.settings.model,
            );
        } else {
            catalog.save_llm_provider_model_config(
                candidate.provider,
                &candidate.settings.api_key,
                &candidate.settings.model,
            );
        }
        catalog.select_llm_provider(candidate.provider);
        store.save_catalog(&catalog)
    })
}

fn provider_base_url(ui: &AppWindow, provider: LlmProviderPreset) -> String {
    match provider {
        LlmProviderPreset::SenseNova | LlmProviderPreset::DeepSeek => {
            provider.base_url().to_owned()
        }
        LlmProviderPreset::Custom => ui.get_custom_llm_base_url().trim().to_owned(),
    }
}

fn validation_error_text(ui: &AppWindow) -> slint::SharedString {
    let translations = ui.global::<Translations>();
    let provider = provider_preset(ui.get_llm_provider());
    if provider != LlmProviderPreset::Custom
        && match provider {
            LlmProviderPreset::SenseNova => ui.get_sensenova_api_key().trim().is_empty(),
            LlmProviderPreset::DeepSeek => ui.get_deepseek_api_key().trim().is_empty(),
            LlmProviderPreset::Custom => false,
        }
    {
        translations.get_models_enter_api_key()
    } else if provider == LlmProviderPreset::Custom
        && ui.get_custom_llm_base_url().trim().is_empty()
    {
        translations.get_models_enter_service_url()
    } else {
        translations.get_models_choose_model()
    }
}

fn apply_validation_error(ui: &AppWindow) {
    ui.set_llm_draft_error(true);
    ui.set_llm_config_status(validation_error_text(ui));
}

#[cfg(test)]
mod tests {
    use std::fs;

    use template_app::ProviderConfigStore;
    use uuid::Uuid;

    use super::*;

    #[test]
    fn failed_connection_test_keeps_the_current_configuration() {
        let directory = std::env::temp_dir().join(format!("saymore-llm-atomic-{}", Uuid::new_v4()));
        let store = JsonSettingsStore::at_path(directory.join("providers.json"));
        let current = LlmCandidate {
            provider: LlmProviderPreset::DeepSeek,
            settings: LlmProviderPreset::DeepSeek.settings("current-key"),
        };
        assert!(save_candidate(&store, &current).is_ok());
        let replacement = LlmCandidate {
            provider: LlmProviderPreset::DeepSeek,
            settings: LlmProviderPreset::DeepSeek.settings("candidate-key"),
        };

        assert!(
            test_save_and_enable(&store, &replacement, |_| {
                Err("connection failed".to_owned())
            })
            .is_err()
        );
        assert_eq!(
            Some("current-key".to_owned()),
            store.load_catalog().ok().and_then(|catalog| catalog
                .llm_provider_api_key(LlmProviderPreset::DeepSeek)
                .map(str::to_owned))
        );
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn successful_connection_test_saves_and_enables_the_candidate() {
        let directory = std::env::temp_dir().join(format!("saymore-llm-enable-{}", Uuid::new_v4()));
        let store = JsonSettingsStore::at_path(directory.join("providers.json"));
        let candidate = LlmCandidate {
            provider: LlmProviderPreset::DeepSeek,
            settings: ChatCompletionsLlmSettings {
                model: "deepseek-chat".to_owned(),
                ..LlmProviderPreset::DeepSeek.settings("candidate-key")
            },
        };

        assert!(test_save_and_enable(&store, &candidate, |_| Ok(())).is_ok());
        let Ok(settings) = store.load() else {
            panic!("enabled LLM settings should remain readable");
        };
        assert!(settings.llm.enabled);
        assert_eq!(candidate.settings.base_url, settings.llm.confirmed_base_url);
        assert_eq!(
            candidate.settings.api_key,
            settings.llm.chat_completions.api_key
        );
        assert_eq!(
            candidate.settings.model,
            settings.llm.chat_completions.model
        );
        let _ = fs::remove_dir_all(directory);
    }
}
