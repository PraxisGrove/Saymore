use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use template_app::LlmProviderPreset;
use template_infra::{JsonSettingsStore, ModelDiscoveryError, discover_models};

use super::{VOLCENGINE_MODELS, provider_preset};

#[path = "model_discovery/catalog.rs"]
mod catalog;
use crate::ui::{AppWindow, AsrProvider as UiAsrProvider, Translations};
pub(super) use catalog::restore_llm_model_catalog;

#[derive(Clone, Copy, PartialEq, Eq)]
enum DiscoveryTarget {
    Volcengine,
    CustomAsr,
    Llm(LlmProviderPreset),
}

struct DiscoveryRequest {
    target: DiscoveryTarget,
    endpoint: String,
    api_key: String,
}

pub(super) fn wire(ui: &AppWindow, store: Arc<JsonSettingsStore>) {
    let generation = Arc::new(AtomicU64::new(0));
    let invalidate_ui = ui.as_weak();
    let invalidate_generation = Arc::clone(&generation);
    ui.on_invalidate_model_discovery(move || {
        invalidate_generation.fetch_add(1, Ordering::Relaxed);
        if let Some(ui) = invalidate_ui.upgrade() {
            ui.set_model_discovery_loading(false);
            ui.set_model_discovery_error(false);
            ui.set_model_discovery_status(SharedString::default());
        }
    });
    let discovery_ui = ui.as_weak();
    let discovery_store = Arc::clone(&store);
    ui.on_refresh_models(move || {
        let Some(ui) = discovery_ui.upgrade() else {
            return;
        };
        if ui.get_model_discovery_loading() {
            return;
        }
        if let Some(request) = prepare_discovery(&ui) {
            start_discovery(
                &ui,
                Arc::clone(&discovery_store),
                request,
                Arc::clone(&generation),
            );
        }
    });

    catalog::wire_selection(ui, store);
}

fn prepare_discovery(ui: &AppWindow) -> Option<DiscoveryRequest> {
    let tab = ui.get_model_tab();
    if tab == 0 && ui.get_asr_provider() == UiAsrProvider::Volcengine {
        apply_models(
            ui,
            DiscoveryTarget::Volcengine,
            VOLCENGINE_MODELS.iter().map(ToString::to_string).collect(),
        );
        return None;
    }
    let target = if tab == 0 {
        DiscoveryTarget::CustomAsr
    } else {
        DiscoveryTarget::Llm(provider_preset(ui.get_llm_provider()))
    };
    if let DiscoveryTarget::Llm(provider) = target
        && provider != LlmProviderPreset::Custom
        && !provider.supports_remote_model_discovery()
    {
        apply_models(
            ui,
            target,
            recommended_models(provider, &ui.get_llm_draft_model()),
        );
        ui.set_model_discovery_status(ui.global::<Translations>().get_models_fetch_presets());
        return None;
    }
    let api_key = selected_api_key(ui, tab);
    if api_key.trim().is_empty()
        && !(target == DiscoveryTarget::Llm(LlmProviderPreset::Custom)
            && template_app::provider_is_local(&ui.get_llm_draft_base_url()))
    {
        discovery_input_error(
            ui,
            ui.global::<Translations>().get_models_fetch_enter_api_key(),
        );
        return None;
    }
    let endpoint = match target {
        DiscoveryTarget::Volcengine => return None,
        DiscoveryTarget::CustomAsr => ui.get_custom_asr_base_url(),
        DiscoveryTarget::Llm(LlmProviderPreset::Custom) => ui.get_llm_draft_base_url(),
        DiscoveryTarget::Llm(provider) => {
            return Some(DiscoveryRequest {
                target,
                endpoint: provider.model_list_url().to_owned(),
                api_key: api_key.to_string(),
            });
        }
    };
    if endpoint.trim().is_empty() {
        discovery_input_error(
            ui,
            ui.global::<Translations>()
                .get_models_fetch_enter_service_url(),
        );
        return None;
    }
    Some(DiscoveryRequest {
        target,
        endpoint: model_list_endpoint(&endpoint),
        api_key: api_key.to_string(),
    })
}

fn model_list_endpoint(base_url: &str) -> String {
    LlmProviderPreset::Custom.model_list_url_for(base_url)
}

fn recommended_models(provider: LlmProviderPreset, selected_model: &str) -> Vec<String> {
    let mut models = provider
        .recommended_models()
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let selected_model = selected_model.trim();
    if !selected_model.is_empty() && !models.iter().any(|model| model == selected_model) {
        models.push(selected_model.to_owned());
    }
    models
}

fn selected_api_key(ui: &AppWindow, tab: i32) -> SharedString {
    if tab == 0 {
        if ui.get_asr_provider() == UiAsrProvider::Custom {
            ui.get_custom_asr_api_key()
        } else {
            ui.get_asr_api_key()
        }
    } else {
        ui.get_llm_draft_api_key()
    }
}

fn discovery_input_error(ui: &AppWindow, status: SharedString) {
    ui.set_model_discovery_status(status);
    ui.set_model_discovery_error(false);
}

fn start_discovery(
    ui: &AppWindow,
    store: Arc<JsonSettingsStore>,
    request: DiscoveryRequest,
    generation: Arc<AtomicU64>,
) {
    let request_generation = generation.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
    ui.set_model_discovery_loading(true);
    ui.set_model_discovery_error(false);
    ui.set_model_discovery_status(ui.global::<Translations>().get_models_fetching());
    let request_ui = ui.as_weak();
    let spawn_result = std::thread::Builder::new()
        .name("saymore-model-discovery".to_owned())
        .spawn(move || {
            let result = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| ModelDiscoveryError::Transport(error.to_string()))
                .and_then(|runtime| {
                    runtime.block_on(discover_models(&request.endpoint, &request.api_key))
                });
            let _ = request_ui.upgrade_in_event_loop(move |ui| {
                if generation.load(Ordering::Relaxed) != request_generation
                    || !request_is_current(&ui, &request)
                {
                    return;
                }
                match result {
                    Ok(models) => catalog::apply_discovered_models(&ui, &store, &request, models),
                    Err(error) => apply_error(&ui, error),
                }
            });
        });
    if spawn_result.is_err() {
        apply_error(
            ui,
            ModelDiscoveryError::Transport("model discovery worker failed".to_owned()),
        );
    }
}

fn target_is_current(ui: &AppWindow, target: DiscoveryTarget) -> bool {
    match target {
        DiscoveryTarget::Volcengine => false,
        DiscoveryTarget::CustomAsr => {
            ui.get_model_tab() == 0 && ui.get_asr_provider() == UiAsrProvider::Custom
        }
        DiscoveryTarget::Llm(provider) => {
            ui.get_model_tab() == 1
                && ui.get_llm_modal_visible()
                && provider_preset(ui.get_llm_provider()) == provider
        }
    }
}

fn request_is_current(ui: &AppWindow, request: &DiscoveryRequest) -> bool {
    if !target_is_current(ui, request.target) {
        return false;
    }
    let tab = ui.get_model_tab();
    if selected_api_key(ui, tab).trim() != request.api_key.trim() {
        return false;
    }
    let endpoint = match request.target {
        DiscoveryTarget::Volcengine => return false,
        DiscoveryTarget::CustomAsr => model_list_endpoint(&ui.get_custom_asr_base_url()),
        DiscoveryTarget::Llm(LlmProviderPreset::Custom) => {
            model_list_endpoint(&ui.get_llm_draft_base_url())
        }
        DiscoveryTarget::Llm(provider) => provider.model_list_url().to_owned(),
    };
    endpoint == request.endpoint
}

fn apply_models(ui: &AppWindow, target: DiscoveryTarget, models: Vec<String>) {
    let selected = selected_model(ui, target, &models);
    match target {
        DiscoveryTarget::Volcengine => ui.set_asr_model(SharedString::from(selected)),
        DiscoveryTarget::CustomAsr => ui.set_custom_asr_model(SharedString::from(selected)),
        DiscoveryTarget::Llm(_) => ui.set_llm_draft_model(SharedString::from(selected)),
    }
    apply_model_list(ui, models);
}

fn selected_model(ui: &AppWindow, target: DiscoveryTarget, models: &[String]) -> String {
    let current = match target {
        DiscoveryTarget::Volcengine => ui.get_asr_model(),
        DiscoveryTarget::CustomAsr => ui.get_custom_asr_model(),
        DiscoveryTarget::Llm(_) => ui.get_llm_draft_model(),
    };
    if models.iter().any(|model| model == current.as_str()) {
        current.to_string()
    } else {
        models.first().cloned().unwrap_or_default()
    }
}

fn apply_model_list(ui: &AppWindow, models: Vec<String>) {
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

#[cfg(test)]
mod tests {
    use template_app::LlmProviderPreset;

    use super::{model_list_endpoint, recommended_models};

    #[test]
    fn custom_model_list_endpoint_normalizes_surrounding_whitespace_and_slashes() {
        assert_eq!(
            "https://asr.example/v1/models",
            model_list_endpoint("  https://asr.example/v1///  ")
        );
    }

    #[test]
    fn model_discovery_accepts_the_same_full_urls_as_provider_configuration() {
        for base_url in [
            "https://provider.example/v1",
            "https://provider.example/v1/chat/completions/",
            "https://provider.example/v1/audio/transcriptions/",
            "https://provider.example/v1/models/",
        ] {
            assert_eq!(
                "https://provider.example/v1/models",
                model_list_endpoint(base_url)
            );
        }
    }

    #[test]
    fn refreshing_a_static_catalog_preserves_a_manually_entered_model() {
        assert_eq!(
            vec![
                "qwen-plus".to_owned(),
                "qwen-flash".to_owned(),
                "workspace-model".to_owned(),
            ],
            recommended_models(LlmProviderPreset::Qwen, " workspace-model ")
        );
    }

    #[test]
    fn minimax_recommendations_preserve_a_manually_entered_model() {
        assert_eq!(
            vec![
                "MiniMax-M3".to_owned(),
                "MiniMax-M2.7".to_owned(),
                "MiniMax-custom".to_owned(),
            ],
            recommended_models(LlmProviderPreset::MiniMax, "MiniMax-custom")
        );
    }
}
