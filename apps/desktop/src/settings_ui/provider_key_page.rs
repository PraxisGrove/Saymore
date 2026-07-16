use std::process::Command;

use slint::ComponentHandle;

use crate::ui::{AppWindow, LlmProvider as UiLlmProvider, Translations};

pub(super) const VOLCENGINE_KEY_PAGE: &str =
    "https://console.volcengine.com/ark/region:ark+cn-beijing/apiKey";
pub(super) const SENSENOVA_KEY_PAGE: &str = "https://platform.sensenova.cn/console/keys";
pub(super) const DEEPSEEK_KEY_PAGE: &str = "https://platform.deepseek.com/api_keys";

pub(super) fn wire(ui: &AppWindow) {
    let weak_ui = ui.as_weak();
    ui.on_open_current_provider_key_page(move || {
        let Some(ui) = weak_ui.upgrade() else {
            return;
        };
        let url = url(ui.get_model_tab(), ui.get_llm_provider());
        if Command::new("/usr/bin/open").arg(url).spawn().is_err() {
            let message = ui
                .global::<Translations>()
                .get_models_open_key_page_failed();
            if ui.get_model_tab() == 0 {
                ui.set_asr_config_status(message);
            } else {
                ui.set_llm_config_status(message);
            }
        }
    });
}

pub(super) fn url(model_tab: i32, llm_provider: UiLlmProvider) -> &'static str {
    if model_tab == 0 {
        VOLCENGINE_KEY_PAGE
    } else {
        match llm_provider {
            UiLlmProvider::Sensenova => SENSENOVA_KEY_PAGE,
            UiLlmProvider::Deepseek => DEEPSEEK_KEY_PAGE,
        }
    }
}
