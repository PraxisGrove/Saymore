use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use template_app::LlmProviderPreset;
use template_infra::{ModelDiscoveryError, discover_models};

use super::{VOLCENGINE_MODELS, provider_preset};
use crate::ui::{
    AppWindow, AsrProvider as UiAsrProvider, LlmProvider as UiLlmProvider, Translations,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum DiscoveryTarget {
    Volcengine,
    CustomAsr,
    Llm(LlmProviderPreset),
}

pub(super) fn wire(ui: &AppWindow) {
    let discovery_ui = ui.as_weak();
    ui.on_refresh_models(move || {
        let Some(ui) = discovery_ui.upgrade() else {
            return;
        };
        let tab = ui.get_model_tab();
        let api_key = if tab == 0 {
            if ui.get_asr_provider() == UiAsrProvider::Custom {
                ui.get_custom_asr_api_key()
            } else {
                ui.get_asr_api_key()
            }
        } else {
            match ui.get_llm_provider() {
                UiLlmProvider::Sensenova => ui.get_sensenova_api_key(),
                UiLlmProvider::Deepseek => ui.get_deepseek_api_key(),
                UiLlmProvider::Custom => ui.get_custom_llm_api_key(),
            }
        };
        if tab == 0 && ui.get_asr_provider() == UiAsrProvider::Volcengine {
            apply_models(
                &ui,
                DiscoveryTarget::Volcengine,
                VOLCENGINE_MODELS.iter().map(ToString::to_string).collect(),
            );
            return;
        }
        let custom_llm = tab == 1 && ui.get_llm_provider() == UiLlmProvider::Custom;
        if api_key.trim().is_empty() && !custom_llm {
            ui.set_model_discovery_status(
                ui.global::<Translations>().get_models_fetch_enter_api_key(),
            );
            ui.set_model_discovery_error(false);
            return;
        }
        if tab == 0
            && ui.get_asr_provider() == UiAsrProvider::Custom
            && ui.get_custom_asr_base_url().trim().is_empty()
        {
            ui.set_model_discovery_status(
                ui.global::<Translations>()
                    .get_models_fetch_enter_service_url(),
            );
            ui.set_model_discovery_error(false);
            return;
        }
        if custom_llm && ui.get_custom_llm_base_url().trim().is_empty() {
            ui.set_model_discovery_status(
                ui.global::<Translations>()
                    .get_models_fetch_enter_service_url(),
            );
            ui.set_model_discovery_error(false);
            return;
        }
        ui.set_available_models(ModelRc::default());
        ui.set_model_discovery_loading(true);
        ui.set_model_discovery_error(false);
        ui.set_model_discovery_status(ui.global::<Translations>().get_models_fetching());
        let target = if tab == 0 {
            DiscoveryTarget::CustomAsr
        } else {
            DiscoveryTarget::Llm(provider_preset(ui.get_llm_provider()))
        };
        let endpoint = match target {
            DiscoveryTarget::Volcengine => return,
            DiscoveryTarget::CustomAsr => {
                format!(
                    "{}/models",
                    ui.get_custom_asr_base_url().trim().trim_end_matches('/')
                )
            }
            DiscoveryTarget::Llm(LlmProviderPreset::Custom) => format!(
                "{}/models",
                ui.get_custom_llm_base_url().trim().trim_end_matches('/')
            ),
            DiscoveryTarget::Llm(provider) => provider.model_list_url().to_owned(),
        };
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
                    let target_is_current = match target {
                        DiscoveryTarget::Volcengine => false,
                        DiscoveryTarget::CustomAsr => {
                            ui.get_model_tab() == 0
                                && ui.get_asr_provider() == UiAsrProvider::Custom
                        }
                        DiscoveryTarget::Llm(provider) => {
                            ui.get_model_tab() == 1
                                && provider_preset(ui.get_llm_provider()) == provider
                        }
                    };
                    if !target_is_current {
                        return;
                    }
                    match result {
                        Ok(models) => apply_models(&ui, target, models),
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

fn apply_models(ui: &AppWindow, target: DiscoveryTarget, models: Vec<String>) {
    let current = match target {
        DiscoveryTarget::Volcengine => ui.get_asr_model(),
        DiscoveryTarget::CustomAsr => ui.get_custom_asr_model(),
        DiscoveryTarget::Llm(LlmProviderPreset::SenseNova) => ui.get_sensenova_model(),
        DiscoveryTarget::Llm(LlmProviderPreset::DeepSeek) => ui.get_deepseek_model(),
        DiscoveryTarget::Llm(LlmProviderPreset::Custom) => ui.get_custom_llm_model(),
    };
    if !models.iter().any(|model| model == current.as_str())
        && let Some(first) = models.first()
    {
        match target {
            DiscoveryTarget::Volcengine => ui.set_asr_model(SharedString::from(first)),
            DiscoveryTarget::CustomAsr => ui.set_custom_asr_model(SharedString::from(first)),
            DiscoveryTarget::Llm(LlmProviderPreset::SenseNova) => {
                ui.set_sensenova_model(SharedString::from(first));
            }
            DiscoveryTarget::Llm(LlmProviderPreset::DeepSeek) => {
                ui.set_deepseek_model(SharedString::from(first));
            }
            DiscoveryTarget::Llm(LlmProviderPreset::Custom) => {
                ui.set_custom_llm_model(SharedString::from(first));
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
    ui.set_model_discovery_error(false);
    ui.set_model_discovery_status(
        ui.global::<Translations>()
            .invoke_models_fetched(i32::try_from(count).unwrap_or(i32::MAX)),
    );
}

fn apply_error(ui: &AppWindow, error: ModelDiscoveryError) {
    let translations = ui.global::<Translations>();
    let status = match error {
        ModelDiscoveryError::MissingApiKey => translations.get_models_fetch_enter_api_key(),
        ModelDiscoveryError::Authentication => translations.get_models_fetch_authentication(),
        ModelDiscoveryError::RateLimited => translations.get_models_fetch_rate_limited(),
        ModelDiscoveryError::Empty => translations.get_models_fetch_empty(),
        ModelDiscoveryError::Transport(_) => translations.get_models_fetch_transport(),
        ModelDiscoveryError::Protocol(_) => translations.get_models_fetch_protocol(),
    };
    ui.set_model_discovery_loading(false);
    ui.set_model_discovery_error(true);
    ui.set_model_discovery_status(status);
}
