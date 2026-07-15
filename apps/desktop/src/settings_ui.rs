use std::sync::Arc;

use slint::{ComponentHandle, SharedString};
use template_app::{
    LlmProviderPreset, ProviderConfigStore, SettingsStore, SettingsStoreError,
    VolcengineAsrSettings,
};
#[cfg(test)]
use template_app::{ProviderCatalog, ProviderInstance, SaymoreSettings};
#[cfg(test)]
use template_infra::AppEnvironment;
use template_infra::{ChatCompletionsLlmProvider, JsonSettingsStore};

use crate::ui::{AppWindow, LlmProvider as UiLlmProvider};

const VOLCENGINE_MODEL: &str = "bigmodel_async";
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

    let save_ui = ui.as_weak();
    let save_store = Arc::clone(&store);
    ui.on_save_asr_config(move |api_key, model| {
        let Some(ui) = save_ui.upgrade() else {
            return;
        };
        match save_asr_configuration(&save_store, api_key.as_str(), model.as_str()) {
            Ok(()) => apply_status(&ui, true, false, "已保存"),
            Err(AsrConfigError::MissingApiKey) => {
                apply_status(&ui, false, true, "请输入 API Key");
            }
            Err(AsrConfigError::MissingModel) => {
                apply_status(&ui, false, true, "请输入模型名称");
            }
            Err(AsrConfigError::Store) => apply_status(&ui, false, true, "保存失败"),
        }
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
                apply_status(&ui, false, false, "未配置");
            }
            Err(_) => apply_status(&ui, true, true, "删除失败"),
        }
    });

    wire_llm(ui, store);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AsrConfigError {
    MissingApiKey,
    MissingModel,
    Store,
}

fn save_asr_configuration(
    store: &JsonSettingsStore,
    api_key: &str,
    model: &str,
) -> Result<(), AsrConfigError> {
    let api_key = api_key.trim();
    let model = model.trim();
    if api_key.is_empty() {
        return Err(AsrConfigError::MissingApiKey);
    }
    if model.is_empty() {
        return Err(AsrConfigError::MissingModel);
    }
    store
        .load()
        .and_then(|mut settings| {
            settings.asr.volcengine = VolcengineAsrSettings {
                enabled: true,
                api_key: api_key.to_owned(),
                model: model.to_owned(),
            };
            store.save(&settings)
        })
        .map_err(|_| AsrConfigError::Store)
}

fn clear_asr_configuration(store: &JsonSettingsStore) -> Result<(), SettingsStoreError> {
    store.load().and_then(|mut settings| {
        settings.asr.volcengine = VolcengineAsrSettings::default();
        store.save(&settings)
    })
}

fn wire_llm(ui: &AppWindow, store: Arc<JsonSettingsStore>) {
    wire_llm_config_actions(ui, Arc::clone(&store));
    wire_llm_enablement(ui, store);
}

fn wire_llm_config_actions(ui: &AppWindow, store: Arc<JsonSettingsStore>) {
    let save_ui = ui.as_weak();
    let save_store = Arc::clone(&store);
    ui.on_save_llm_config(move |provider, api_key| {
        let Some(ui) = save_ui.upgrade() else {
            return;
        };
        let provider = provider_preset(provider);
        if api_key.trim().is_empty() {
            ui.set_llm_config_status(SharedString::from("请输入 API Key"));
            return;
        }
        let result = save_store.load_catalog().and_then(|mut catalog| {
            catalog.save_llm_provider_config(provider, api_key.as_str());
            save_store.save_catalog(&catalog)
        });
        if result.is_ok() {
            apply_loaded_settings(&ui, &save_store);
            ui.set_llm_config_status(SharedString::from("已保存，请启用并测试连接"));
        } else {
            ui.set_llm_config_status(SharedString::from("保存失败"));
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
            ui.set_llm_config_status(SharedString::from("切换失败"));
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
            ui.set_llm_config_status(SharedString::from("配置读取失败"));
            return;
        };
        if catalog
            .llm_provider_api_key(provider)
            .is_none_or(str::is_empty)
        {
            ui.set_llm_config_status(SharedString::from("请先保存 API Key"));
            return;
        }
        let base_url = provider.base_url().to_owned();
        let local = provider_is_local(&base_url);
        ui.set_llm_provider_target(SharedString::from(&base_url));
        ui.set_llm_provider_local(local);
        ui.set_llm_confirmation_visible(true);
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
            ui.set_llm_config_status(SharedString::from(if result.is_ok() {
                "未启用"
            } else {
                "保存失败"
            }));
            return;
        }
        let provider = provider_preset(ui.get_llm_provider());
        if provider.base_url() != expected_base_url.trim() {
            ui.set_llm_config_status(SharedString::from("模型提供商已改变"));
            return;
        }
        start_llm_test(&ui, Arc::clone(&store), provider);
    });
}

fn start_llm_test(ui: &AppWindow, store: Arc<JsonSettingsStore>, provider: LlmProviderPreset) {
    ui.set_llm_enabled(false);
    ui.set_llm_config_status(SharedString::from("正在测试连接"));
    let test_ui = ui.as_weak();
    let spawn_result = std::thread::Builder::new()
        .name("saymore-test-llm".to_owned())
        .spawn(move || {
            let result = test_and_enable_llm(&store, provider);
            let event_store = Arc::clone(&store);
            let _ = test_ui.upgrade_in_event_loop(move |ui| {
                apply_loaded_settings(&ui, &event_store);
                if result.is_err() {
                    ui.set_llm_config_status(SharedString::from("连接测试失败"));
                }
            });
        });
    if spawn_result.is_err() {
        ui.set_llm_config_status(SharedString::from("连接测试失败"));
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

fn apply_loaded_settings(ui: &AppWindow, store: &JsonSettingsStore) {
    match (store.load(), store.load_catalog()) {
        (Ok(settings), Ok(catalog)) => {
            let selected = catalog
                .active_llm_provider()
                .unwrap_or(LlmProviderPreset::SenseNova);
            ui.set_llm_provider(ui_provider(selected));
            ui.set_sensenova_api_key(SharedString::from(
                catalog
                    .llm_provider_api_key(LlmProviderPreset::SenseNova)
                    .unwrap_or_default(),
            ));
            ui.set_deepseek_api_key(SharedString::from(
                catalog
                    .llm_provider_api_key(LlmProviderPreset::DeepSeek)
                    .unwrap_or_default(),
            ));
            let sensenova_model =
                catalog.configured_llm_provider_model(LlmProviderPreset::SenseNova);
            let deepseek_model = catalog.configured_llm_provider_model(LlmProviderPreset::DeepSeek);
            ui.set_sensenova_configured(sensenova_model.is_some());
            ui.set_deepseek_configured(deepseek_model.is_some());
            ui.set_sensenova_model(SharedString::from(
                sensenova_model.unwrap_or_else(|| LlmProviderPreset::SenseNova.model()),
            ));
            ui.set_deepseek_model(SharedString::from(
                deepseek_model.unwrap_or_else(|| LlmProviderPreset::DeepSeek.model()),
            ));
            let llm_configured = !settings.llm.chat_completions.api_key.trim().is_empty();
            let llm_base_url = selected.base_url();
            let llm_enabled = settings.llm.enabled
                && llm_configured
                && settings.llm.confirmed_base_url.trim() == llm_base_url;
            ui.set_llm_enabled(llm_enabled);
            ui.set_llm_provider_target(SharedString::from(llm_base_url));
            ui.set_llm_provider_local(provider_is_local(llm_base_url));
            ui.set_llm_config_status(SharedString::from(if llm_enabled {
                "已启用"
            } else if llm_configured {
                "未启用"
            } else {
                "请保存当前提供商的 API Key"
            }));
            let provider = settings.asr.volcengine;
            let configured = provider.enabled && !provider.api_key.trim().is_empty();
            ui.set_asr_api_key(SharedString::from(provider.api_key));
            ui.set_asr_model(SharedString::from(if provider.model.trim().is_empty() {
                VOLCENGINE_MODEL
            } else {
                provider.model.as_str()
            }));
            apply_status(
                ui,
                configured,
                false,
                if configured { "已配置" } else { "未配置" },
            );
        }
        _ => apply_status(ui, false, true, "配置读取失败"),
    }
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

fn apply_status(ui: &AppWindow, configured: bool, error: bool, status: &str) {
    ui.set_asr_configured(configured);
    ui.set_asr_config_error(error);
    ui.set_asr_config_status(SharedString::from(status));
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
    fn asr_configuration_rejects_an_empty_model() {
        let directory =
            std::env::temp_dir().join(format!("saymore-asr-validation-{}", Uuid::new_v4()));
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
        let directory =
            std::env::temp_dir().join(format!("saymore-asr-round-trip-{}", Uuid::new_v4()));
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
    #[ignore = "uses and enables the current user's live SenseNova configuration"]
    fn current_user_sensenova_configuration_can_be_enabled() {
        let Ok(store) = JsonSettingsStore::for_current_user(AppEnvironment::Production) else {
            panic!("current user settings should be available");
        };

        let result = test_and_enable_llm(&store, LlmProviderPreset::SenseNova);

        assert!(result.is_ok(), "SenseNova enablement failed: {result:?}");
    }
}
