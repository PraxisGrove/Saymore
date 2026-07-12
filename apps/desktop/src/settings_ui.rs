use std::sync::Arc;

use slint::{ComponentHandle, SharedString};
use template_app::{SettingsStore, VolcengineAsrSettings};
use template_infra::JsonSettingsStore;

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
    ui.on_delete_asr_config(move || {
        let Some(ui) = delete_ui.upgrade() else {
            return;
        };
        let result = store.load().and_then(|mut settings| {
            settings.asr.volcengine = VolcengineAsrSettings::default();
            store.save(&settings)
        });
        match result {
            Ok(()) => {
                ui.set_asr_api_key(SharedString::new());
                apply_status(&ui, false, false, "未配置");
            }
            Err(_) => apply_status(&ui, true, true, "删除失败"),
        }
    });
}

fn apply_loaded_settings(ui: &AppWindow, store: &JsonSettingsStore) {
    match store.load() {
        Ok(settings) => {
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

fn apply_status(ui: &AppWindow, configured: bool, error: bool, status: &str) {
    ui.set_asr_configured(configured);
    ui.set_asr_config_error(error);
    ui.set_asr_config_status(SharedString::from(status));
}
