use slint::ComponentHandle;

use crate::{
    platform_open,
    ui::{AppWindow, LlmProvider as UiLlmProvider, Translations},
};

pub(super) const VOLCENGINE_KEY_PAGE: &str =
    "https://console.volcengine.com/ark/region:ark+cn-beijing/apiKey";
pub(super) const SENSENOVA_KEY_PAGE: &str = "https://platform.sensenova.cn/console/keys";
pub(super) const DEEPSEEK_KEY_PAGE: &str = "https://platform.deepseek.com/api_keys";
pub(super) const QWEN_KEY_PAGE: &str = "https://bailian.console.aliyun.com/?apiKey=1#/api-key";
pub(super) const ZHIPU_KEY_PAGE: &str = "https://bigmodel.cn/usercenter/proj-mgmt/apikeys";
pub(super) const MINIMAX_KEY_PAGE: &str =
    "https://platform.minimaxi.com/console/access?tab=api-keys";
pub(super) const OPENAI_KEY_PAGE: &str = "https://platform.openai.com/api-keys";
pub(super) const KIMI_KEY_PAGE: &str = "https://platform.moonshot.cn/console/api-keys";
pub(super) const GEMINI_KEY_PAGE: &str = "https://aistudio.google.com/app/apikey";
pub(super) const OPENROUTER_KEY_PAGE: &str = "https://openrouter.ai/settings/keys";
pub(super) const SILICONFLOW_KEY_PAGE: &str = "https://cloud.siliconflow.cn/account/ak";
pub(super) const STEPFUN_KEY_PAGE: &str = "https://platform.stepfun.com/interface-key";

pub(super) fn wire(ui: &AppWindow) {
    let weak_ui = ui.as_weak();
    ui.on_open_current_provider_key_page(move || {
        let Some(ui) = weak_ui.upgrade() else {
            return;
        };
        let Some(url) = url(ui.get_model_tab(), ui.get_llm_provider()) else {
            return;
        };
        if platform_open::open(url).is_err() {
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

pub(super) fn url(model_tab: i32, llm_provider: UiLlmProvider) -> Option<&'static str> {
    if model_tab == 0 {
        Some(VOLCENGINE_KEY_PAGE)
    } else {
        match llm_provider {
            UiLlmProvider::Sensenova => Some(SENSENOVA_KEY_PAGE),
            UiLlmProvider::Deepseek => Some(DEEPSEEK_KEY_PAGE),
            UiLlmProvider::Qwen => Some(QWEN_KEY_PAGE),
            UiLlmProvider::VolcengineArk => Some(VOLCENGINE_KEY_PAGE),
            UiLlmProvider::Openai => Some(OPENAI_KEY_PAGE),
            UiLlmProvider::Kimi => Some(KIMI_KEY_PAGE),
            UiLlmProvider::Gemini => Some(GEMINI_KEY_PAGE),
            UiLlmProvider::Openrouter => Some(OPENROUTER_KEY_PAGE),
            UiLlmProvider::ZhipuGlm => Some(ZHIPU_KEY_PAGE),
            UiLlmProvider::Minimax => Some(MINIMAX_KEY_PAGE),
            UiLlmProvider::Siliconflow => Some(SILICONFLOW_KEY_PAGE),
            UiLlmProvider::Stepfun => Some(STEPFUN_KEY_PAGE),
            UiLlmProvider::Custom => None,
        }
    }
}
