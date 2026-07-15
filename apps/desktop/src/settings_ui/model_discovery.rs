use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use template_app::LlmProviderPreset;
use template_infra::{ModelDiscoveryError, discover_models};

use super::{VOLCENGINE_MODEL, provider_preset};
use crate::ui::{AppWindow, LlmProvider as UiLlmProvider};

pub(super) fn wire(ui: &AppWindow) {
    let discovery_ui = ui.as_weak();
    ui.on_refresh_models(move || {
        let Some(ui) = discovery_ui.upgrade() else {
            return;
        };
        let tab = ui.get_model_tab();
        let api_key = if tab == 0 {
            ui.get_asr_api_key()
        } else if ui.get_llm_provider() == UiLlmProvider::Deepseek {
            ui.get_deepseek_api_key()
        } else {
            ui.get_sensenova_api_key()
        };
        if api_key.trim().is_empty() {
            ui.set_model_discovery_status(SharedString::from("请先输入 API Key"));
            return;
        }
        ui.set_available_models(ModelRc::default());
        ui.set_model_discovery_loading(true);
        ui.set_model_discovery_status(SharedString::from("正在获取"));
        if tab == 0 {
            apply_models(&ui, None, vec![VOLCENGINE_MODEL.to_owned()]);
            return;
        }

        let provider = provider_preset(ui.get_llm_provider());
        let endpoint = provider.model_list_url().to_owned();
        let api_key = api_key.to_string();
        let request_ui = ui.as_weak();
        let spawn_result = std::thread::Builder::new()
            .name("saymore-model-discovery".to_owned())
            .spawn(move || {
                let result = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|error| ModelDiscoveryError::Transport(error.to_string()))
                    .and_then(|runtime| runtime.block_on(discover_models(&endpoint, &api_key)));
                let _ = request_ui.upgrade_in_event_loop(move |ui| {
                    if ui.get_model_tab() != 1 || provider_preset(ui.get_llm_provider()) != provider
                    {
                        return;
                    }
                    match result {
                        Ok(models) => apply_models(&ui, Some(provider), models),
                        Err(error) => apply_error(&ui, error),
                    }
                });
            });
        if spawn_result.is_err() {
            apply_error(
                &ui,
                ModelDiscoveryError::Transport("model discovery worker failed".to_owned()),
            );
        }
    });
}

fn apply_models(ui: &AppWindow, provider: Option<LlmProviderPreset>, models: Vec<String>) {
    let current = match provider {
        None => ui.get_asr_model(),
        Some(LlmProviderPreset::SenseNova) => ui.get_sensenova_model(),
        Some(LlmProviderPreset::DeepSeek) => ui.get_deepseek_model(),
    };
    if !models.iter().any(|model| model == current.as_str())
        && let Some(first) = models.first()
    {
        match provider {
            None => ui.set_asr_model(SharedString::from(first)),
            Some(LlmProviderPreset::SenseNova) => {
                ui.set_sensenova_model(SharedString::from(first));
            }
            Some(LlmProviderPreset::DeepSeek) => {
                ui.set_deepseek_model(SharedString::from(first));
            }
        }
    }
    let count = models.len();
    let models = models
        .into_iter()
        .map(SharedString::from)
        .collect::<Vec<_>>();
    ui.set_available_models(ModelRc::new(VecModel::from(models)));
    ui.set_model_discovery_loading(false);
    ui.set_model_discovery_status(SharedString::from(format!("已获取 {count} 个模型")));
}

fn apply_error(ui: &AppWindow, error: ModelDiscoveryError) {
    let status = match error {
        ModelDiscoveryError::MissingApiKey => "请先输入 API Key",
        ModelDiscoveryError::Authentication => "API Key 无效",
        ModelDiscoveryError::RateLimited => "请求过于频繁",
        ModelDiscoveryError::Empty => "未获取到可用模型",
        ModelDiscoveryError::Transport(_) => "无法连接模型服务",
        ModelDiscoveryError::Protocol(_) => "模型列表响应异常",
    };
    ui.set_model_discovery_loading(false);
    ui.set_model_discovery_status(SharedString::from(status));
}
