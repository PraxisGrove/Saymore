use std::sync::Arc;

use chrono::Utc;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use template_app::{LlmProviderPreset, ProviderCatalog, ProviderConfigStore};
use template_infra::{JsonSettingsStore, ModelDiscoveryError, discover_models};

use super::{VOLCENGINE_MODELS, provider_preset};
use crate::ui::{AppWindow, AsrProvider as UiAsrProvider, Translations};

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
    let discovery_ui = ui.as_weak();
    let discovery_store = Arc::clone(&store);
    ui.on_refresh_models(move || {
        let Some(ui) = discovery_ui.upgrade() else {
            return;
        };
        if let Some(request) = prepare_discovery(&ui) {
            start_discovery(&ui, Arc::clone(&discovery_store), request);
        }
    });

    let selection_ui = ui.as_weak();
    ui.on_select_llm_model(move |provider, model| {
        let Some(ui) = selection_ui.upgrade() else {
            return;
        };
        let provider = provider_preset(provider);
        let Ok(catalog) = store.load_catalog() else {
            apply_selection_error(&ui);
            return;
        };
        let Some(settings) = catalog.llm_provider_settings(provider) else {
            apply_selection_error(&ui);
            return;
        };
        let model_list_url = if provider == LlmProviderPreset::Custom {
            model_list_endpoint(&settings.base_url)
        } else {
            provider.model_list_url().to_owned()
        };
        match store.select_cached_llm_model(provider, &model_list_url, settings.profile, &model) {
            Ok(true) => {
                ui.set_llm_saved_model(model.trim().into());
                ui.set_llm_config_dirty(
                    ui.get_llm_draft_api_key() != ui.get_llm_saved_api_key()
                        || ui.get_llm_draft_base_url() != ui.get_llm_saved_base_url()
                        || ui.get_llm_draft_model() != ui.get_llm_saved_model(),
                );
            }
            Ok(false) | Err(_) => apply_selection_error(&ui),
        }
    });
}

fn apply_selection_error(ui: &AppWindow) {
    ui.set_llm_draft_error(true);
    ui.set_llm_config_status(
        ui.global::<Translations>()
            .get_common_configuration_load_failed(),
    );
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
        return None;
    }
    let api_key = selected_api_key(ui, tab);
    if api_key.trim().is_empty() {
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
    format!("{}/models", base_url.trim().trim_end_matches('/'))
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

fn start_discovery(ui: &AppWindow, store: Arc<JsonSettingsStore>, request: DiscoveryRequest) {
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
                if !request_is_current(&ui, &request) {
                    return;
                }
                match result {
                    Ok(models) => apply_discovered_models(&ui, &store, &request, models),
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

fn apply_discovered_models(
    ui: &AppWindow,
    store: &JsonSettingsStore,
    request: &DiscoveryRequest,
    models: Vec<String>,
) {
    let selected = selected_model(ui, request.target, &models);
    let persistence = match request.target {
        DiscoveryTarget::Llm(provider) => store.cache_llm_model_catalog(
            provider,
            &request.endpoint,
            provider.profile().chat_completions,
            models.clone(),
            &selected,
            Utc::now().timestamp_millis(),
        ),
        DiscoveryTarget::Volcengine | DiscoveryTarget::CustomAsr => Ok(true),
    };
    apply_models(ui, request.target, models);
    if !matches!(persistence, Ok(true)) {
        tracing::warn!(
            target: "saymore::diagnostics",
            event = "llm.model_catalog_save_failed"
        );
        ui.set_model_discovery_error(true);
        ui.set_model_discovery_status(
            ui.global::<Translations>()
                .get_common_configuration_load_failed(),
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
            ui.get_model_tab() == 1 && provider_preset(ui.get_llm_provider()) == provider
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

pub(super) fn restore_llm_model_catalog(
    ui: &AppWindow,
    catalog: &ProviderCatalog,
    provider: LlmProviderPreset,
) {
    let Some(settings) = catalog.llm_provider_settings(provider) else {
        set_model_list(ui, recommended_models(provider, &ui.get_llm_draft_model()));
        return;
    };
    let endpoint = if provider == LlmProviderPreset::Custom {
        model_list_endpoint(&settings.base_url)
    } else {
        provider.model_list_url().to_owned()
    };
    let Some(cached) = catalog.llm_model_catalog(provider, &endpoint, settings.profile) else {
        let mut models = provider
            .recommended_models()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        if !settings.model.trim().is_empty() && !models.iter().any(|model| model == &settings.model)
        {
            models.push(settings.model);
        }
        set_model_list(ui, models);
        return;
    };
    ui.set_llm_draft_model(SharedString::from(cached.selected_model));
    set_model_list(ui, cached.models);
}

fn set_model_list(ui: &AppWindow, models: Vec<String>) {
    let models = models
        .into_iter()
        .map(SharedString::from)
        .collect::<Vec<_>>();
    ui.set_available_models(ModelRc::new(VecModel::from(models)));
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
