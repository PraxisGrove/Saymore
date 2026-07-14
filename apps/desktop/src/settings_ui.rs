use std::sync::Arc;

use slint::{ComponentHandle, SharedString};
use template_app::{SettingsStore, VolcengineAsrSettings};
use template_infra::{ChatCompletionsLlmProvider, JsonSettingsStore};

use crate::ui::AppWindow;

const VOLCENGINE_MODEL: &str = "bigmodel_async";

pub fn wire(ui: &AppWindow, store: Arc<JsonSettingsStore>) {
    apply_loaded_settings(ui, &store);

    let save_ui = ui.as_weak();
    let save_store = Arc::clone(&store);
    ui.on_save_asr_config(move |api_key| {
        let Some(ui) = save_ui.upgrade() else {
            return;
        };
        let api_key = api_key.trim();
        if api_key.is_empty() {
            apply_status(&ui, false, true, "请输入 API Key");
            return;
        }

        let result = save_store.load().and_then(|mut settings| {
            settings.asr.volcengine = VolcengineAsrSettings {
                enabled: true,
                api_key: api_key.to_owned(),
                model: VOLCENGINE_MODEL.to_owned(),
            };
            save_store.save(&settings)
        });
        match result {
            Ok(()) => apply_status(&ui, true, false, "已保存"),
            Err(_) => apply_status(&ui, false, true, "保存失败"),
        }
    });

    let delete_ui = ui.as_weak();
    let delete_store = Arc::clone(&store);
    ui.on_delete_asr_config(move || {
        let Some(ui) = delete_ui.upgrade() else {
            return;
        };
        let result = delete_store.load().and_then(|mut settings| {
            settings.asr.volcengine = VolcengineAsrSettings::default();
            delete_store.save(&settings)
        });
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

fn wire_llm(ui: &AppWindow, store: Arc<JsonSettingsStore>) {
    let prepare_ui = ui.as_weak();
    let prepare_store = Arc::clone(&store);
    ui.on_request_llm_enable(move || {
        let Some(ui) = prepare_ui.upgrade() else {
            return;
        };
        let Ok(settings) = prepare_store.load() else {
            ui.set_llm_config_status(SharedString::from("配置读取失败"));
            return;
        };
        let base_url = settings.llm.chat_completions.base_url.trim().to_owned();
        if base_url.is_empty() {
            ui.set_llm_config_status(SharedString::from("未配置服务地址"));
            return;
        }
        let local = provider_is_local(&base_url);
        ui.set_llm_provider_target(SharedString::from(&base_url));
        ui.set_llm_provider_local(local);
        if local {
            start_llm_test(&ui, Arc::clone(&prepare_store), base_url);
        } else {
            ui.set_llm_confirmation_visible(true);
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
            ui.set_llm_config_status(SharedString::from(if result.is_ok() {
                "未启用"
            } else {
                "保存失败"
            }));
            return;
        }
        start_llm_test(&ui, Arc::clone(&store), expected_base_url.to_string());
    });
}

fn start_llm_test(ui: &AppWindow, store: Arc<JsonSettingsStore>, expected_base_url: String) {
    ui.set_llm_enabled(false);
    ui.set_llm_config_status(SharedString::from("正在测试连接"));
    let test_ui = ui.as_weak();
    let spawn_result = std::thread::Builder::new()
        .name("saymore-test-llm".to_owned())
        .spawn(move || {
            let result = test_and_enable_llm(&store, &expected_base_url);
            let _ = test_ui.upgrade_in_event_loop(move |ui| {
                ui.set_llm_enabled(result.is_ok());
                ui.set_llm_config_status(SharedString::from(if result.is_ok() {
                    "已启用"
                } else {
                    "连接测试失败"
                }));
            });
        });
    if spawn_result.is_err() {
        ui.set_llm_config_status(SharedString::from("连接测试失败"));
    }
}

fn test_and_enable_llm(store: &JsonSettingsStore, expected_base_url: &str) -> Result<(), String> {
    let settings = store.load().map_err(|error| error.to_string())?;
    let provider_settings = settings.llm.chat_completions;
    if provider_settings.base_url.trim() != expected_base_url.trim() {
        return Err("LLM provider changed before confirmation".to_owned());
    }
    let provider = ChatCompletionsLlmProvider::new(provider_settings.clone())
        .map_err(|error| error.to_string())?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    runtime
        .block_on(provider.test_connection())
        .map_err(|error| error.to_string())?;
    let mut settings = store.load().map_err(|error| error.to_string())?;
    if settings.llm.chat_completions != provider_settings {
        return Err("LLM provider changed during connection test".to_owned());
    }
    settings.llm.enabled = true;
    settings.llm.confirmed_base_url = expected_base_url.trim().to_owned();
    store.save(&settings).map_err(|error| error.to_string())
}

fn apply_loaded_settings(ui: &AppWindow, store: &JsonSettingsStore) {
    match store.load() {
        Ok(settings) => {
            let llm_base_url = settings.llm.chat_completions.base_url.trim();
            let llm_enabled = settings.llm.enabled
                && !llm_base_url.is_empty()
                && settings.llm.confirmed_base_url.trim() == llm_base_url;
            ui.set_llm_enabled(llm_enabled);
            ui.set_llm_provider_target(SharedString::from(llm_base_url));
            ui.set_llm_provider_local(provider_is_local(llm_base_url));
            ui.set_llm_config_status(SharedString::from(if llm_enabled {
                "已启用"
            } else {
                "未启用"
            }));
            let provider = settings.asr.volcengine;
            let configured = provider.enabled && !provider.api_key.trim().is_empty();
            ui.set_asr_api_key(SharedString::from(provider.api_key));
            apply_status(
                ui,
                configured,
                false,
                if configured { "已配置" } else { "未配置" },
            );
        }
        Err(_) => apply_status(ui, false, true, "配置读取失败"),
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
