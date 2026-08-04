use std::sync::Arc;

use slint::ComponentHandle;
use template_app::{
    ChatCompletionsLlmSettings, LlmProviderConfiguration, LlmProviderPreset, ProviderConfigStore,
    ProviderConfigurator, SettingsStore, SettingsStoreError, llm_consent_required,
    provider_is_local,
};
use template_infra::{ChatCompletionsLlmProvider, JsonSettingsStore};

use crate::ui::{AppWindow, LlmProvider as UiLlmProvider, Translations};

use super::provider_connection::DesktopProviderConnectionTester;
use super::{apply_loaded_settings, loaded_settings::apply_llm_draft, provider_preset};

#[derive(Debug)]
enum LlmSaveError {
    Invalid,
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
                Ok(candidate) if candidate.settings().base_url == expected_base_url.trim() => {
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

    let prepare_ui = ui.as_weak();
    let prepare_store = Arc::clone(&store);
    ui.on_prepare_llm_provider(move |provider| {
        let Some(ui) = prepare_ui.upgrade() else {
            return;
        };
        if let Ok(catalog) = prepare_store.load_catalog() {
            apply_llm_draft(&ui, &catalog, provider_preset(provider));
        } else {
            ui.set_llm_config_status(
                ui.global::<Translations>()
                    .get_common_configuration_load_failed(),
            );
        }
    });

    let select_ui = ui.as_weak();
    ui.on_select_llm_provider(move |provider| {
        let Some(ui) = select_ui.upgrade() else {
            return;
        };
        let provider = provider_preset(provider);
        match select_configured_llm_provider(&store, provider) {
            Ok(true) => {
                apply_loaded_settings(&ui, &store);
                ui.set_llm_provider_target(provider_base_url(&ui, provider).into());
            }
            Ok(false) | Err(_) => {
                ui.set_llm_config_status(ui.global::<Translations>().get_models_switch_failed());
            }
        }
    });
}

fn select_configured_llm_provider(
    store: &JsonSettingsStore,
    provider: LlmProviderPreset,
) -> Result<bool, SettingsStoreError> {
    let mut catalog = store.load_catalog()?;
    let configured = catalog
        .llm_provider_settings(provider)
        .is_some_and(|settings| super::llm_configuration_ready(provider, &settings));
    if !configured {
        return Ok(false);
    }
    catalog.select_llm_provider(provider);
    store.save_catalog(&catalog)?;
    Ok(true)
}

fn candidate(
    provider: UiLlmProvider,
    api_key: &str,
    base_url: &str,
    model: &str,
) -> Result<LlmProviderConfiguration, LlmSaveError> {
    let provider = provider_preset(provider);
    let api_key = api_key.trim();
    if provider != LlmProviderPreset::Custom && api_key.is_empty() {
        return Err(LlmSaveError::Invalid);
    }
    let base_url = if provider.base_url_editable() {
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
        profile: provider.profile().chat_completions,
    };
    ChatCompletionsLlmProvider::new(settings.clone()).map_err(|_| LlmSaveError::Invalid)?;
    Ok(LlmProviderConfiguration::new(provider, settings))
}

fn begin_activation(
    ui: &AppWindow,
    store: Arc<JsonSettingsStore>,
    candidate: Result<LlmProviderConfiguration, LlmSaveError>,
) {
    let candidate = match candidate {
        Ok(candidate) => candidate,
        Err(_) => {
            apply_validation_error(ui);
            return;
        }
    };
    let base_url = candidate.settings().base_url.as_str();
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

fn start_activation(
    ui: &AppWindow,
    store: Arc<JsonSettingsStore>,
    candidate: LlmProviderConfiguration,
) {
    ui.set_llm_testing(true);
    ui.set_llm_draft_error(false);
    ui.set_llm_config_status(ui.global::<Translations>().get_models_testing_connection());
    let result_ui = ui.as_weak();
    let spawn_result = std::thread::Builder::new()
        .name("saymore-test-llm-config".to_owned())
        .spawn(move || {
            let tester = DesktopProviderConnectionTester;
            let result =
                ProviderConfigurator::new(&tester, store.as_ref()).configure_llm(&candidate);
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

fn provider_base_url(ui: &AppWindow, provider: LlmProviderPreset) -> String {
    if provider.base_url_editable() {
        ui.get_llm_draft_base_url().trim().to_owned()
    } else {
        provider.base_url().to_owned()
    }
}

fn validation_error_text(ui: &AppWindow) -> slint::SharedString {
    let translations = ui.global::<Translations>();
    let provider = provider_preset(ui.get_llm_provider());
    if provider != LlmProviderPreset::Custom && ui.get_llm_draft_api_key().trim().is_empty() {
        translations.get_models_enter_api_key()
    } else if provider.base_url_editable() && ui.get_llm_draft_base_url().trim().is_empty() {
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

    use template_app::ProviderConfigurationStore;
    use uuid::Uuid;

    use super::*;

    #[test]
    fn every_built_in_provider_builds_its_declared_configuration() {
        let providers = [
            (UiLlmProvider::Sensenova, LlmProviderPreset::SenseNova),
            (UiLlmProvider::Deepseek, LlmProviderPreset::DeepSeek),
            (UiLlmProvider::Qwen, LlmProviderPreset::Qwen),
            (
                UiLlmProvider::VolcengineArk,
                LlmProviderPreset::VolcengineArk,
            ),
            (UiLlmProvider::Openai, LlmProviderPreset::OpenAi),
            (UiLlmProvider::Kimi, LlmProviderPreset::Kimi),
            (UiLlmProvider::Gemini, LlmProviderPreset::Gemini),
            (UiLlmProvider::Openrouter, LlmProviderPreset::OpenRouter),
            (UiLlmProvider::ZhipuGlm, LlmProviderPreset::ZhipuGlm),
            (UiLlmProvider::Minimax, LlmProviderPreset::MiniMax),
            (UiLlmProvider::Siliconflow, LlmProviderPreset::SiliconFlow),
            (UiLlmProvider::Stepfun, LlmProviderPreset::StepFun),
        ];

        for (ui_provider, preset) in providers {
            let Ok(candidate) = candidate(
                ui_provider,
                " provider-key ",
                preset.base_url(),
                preset.model(),
            ) else {
                panic!("{} should build a valid UI candidate", preset.label());
            };

            assert_eq!(preset, candidate.provider());
            assert_eq!(preset.base_url(), candidate.settings().base_url);
            assert_eq!(preset.model(), candidate.settings().model);
            assert_eq!("provider-key", candidate.settings().api_key);
            assert_eq!(
                preset.profile().chat_completions,
                candidate.settings().profile
            );
        }
    }

    #[test]
    fn custom_local_provider_candidate_can_omit_an_api_key() {
        let Ok(candidate) = candidate(
            UiLlmProvider::Custom,
            "",
            " http://localhost:11434/v1/ ",
            " local-model ",
        ) else {
            panic!("local custom provider should accept an empty API key");
        };

        assert_eq!(LlmProviderPreset::Custom, candidate.provider());
        assert_eq!("http://localhost:11434/v1", candidate.settings().base_url);
        assert_eq!("local-model", candidate.settings().model);
        assert!(candidate.settings().api_key.is_empty());
    }

    #[test]
    fn previously_enabled_provider_can_be_selected_without_another_connection_test() {
        let directory = std::env::temp_dir().join(format!("saymore-llm-switch-{}", Uuid::new_v4()));
        let store = JsonSettingsStore::at_path(directory.join("providers.json"));
        let deepseek = LlmProviderConfiguration::new(
            LlmProviderPreset::DeepSeek,
            LlmProviderPreset::DeepSeek.settings("deepseek-key"),
        );
        let sensenova = LlmProviderConfiguration::new(
            LlmProviderPreset::SenseNova,
            LlmProviderPreset::SenseNova.settings("sensenova-key"),
        );
        assert!(store.save_and_enable_llm_configuration(&deepseek).is_ok());
        assert!(store.save_and_enable_llm_configuration(&sensenova).is_ok());

        assert_eq!(
            Ok(true),
            select_configured_llm_provider(&store, LlmProviderPreset::DeepSeek)
        );
        let Ok(settings) = store.load() else {
            panic!("reselected DeepSeek settings should remain readable");
        };
        assert!(settings.llm.enabled);
        assert_eq!("deepseek-key", settings.llm.chat_completions.api_key);
        assert_eq!(
            LlmProviderPreset::DeepSeek.base_url(),
            settings.llm.confirmed_base_url
        );
        let _ = fs::remove_dir_all(directory);
    }
}
