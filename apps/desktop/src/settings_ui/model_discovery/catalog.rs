use std::sync::Arc;

use chrono::Utc;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use template_app::{LlmProviderPreset, ProviderCatalog, ProviderConfigStore};
use template_infra::JsonSettingsStore;

use super::{
    DiscoveryRequest, DiscoveryTarget, apply_models, provider_preset, recommended_models,
    selected_model,
};
use crate::ui::{AppWindow, Translations};

pub(super) fn wire_selection(ui: &AppWindow, store: Arc<JsonSettingsStore>) {
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
            return;
        };
        // Keep choices made with unsaved credentials in the draft.
        if (settings.api_key.trim().is_empty()
            && !(provider == LlmProviderPreset::Custom
                && template_app::provider_is_local(&settings.base_url)))
            || ui.get_llm_draft_api_key() != ui.get_llm_saved_api_key()
            || ui.get_llm_draft_base_url() != ui.get_llm_saved_base_url()
        {
            return;
        }
        let model_list_url = provider.model_list_url_for(&settings.base_url);
        if !catalog
            .llm_model_catalog(provider, &model_list_url, settings.profile)
            .is_some_and(|cached| cached.models.iter().any(|id| id == model.trim()))
        {
            return;
        }
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

pub(super) fn apply_discovered_models(
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
    if matches!(persistence, Ok(true)) {
        apply_models(ui, request.target, models);
    } else {
        ui.set_model_discovery_loading(false);
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

pub(in crate::settings_ui) fn restore_llm_model_catalog(
    ui: &AppWindow,
    catalog: &ProviderCatalog,
    provider: LlmProviderPreset,
) {
    let Some(settings) = catalog.llm_provider_settings(provider) else {
        set_model_list(ui, recommended_models(provider, &ui.get_llm_draft_model()));
        return;
    };
    let endpoint = provider.model_list_url_for(&settings.base_url);
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
    // A cached draft choice must not replace a confirmed provider configuration.
    let selected = if settings.model.trim().is_empty() {
        cached.selected_model
    } else {
        settings.model
    };
    let mut models = cached.models;
    if !selected.trim().is_empty() && !models.iter().any(|model| model == &selected) {
        models.push(selected.clone());
    }
    ui.set_llm_draft_model(SharedString::from(selected));
    set_model_list(ui, models);
}

fn set_model_list(ui: &AppWindow, models: Vec<String>) {
    let models = models
        .into_iter()
        .map(SharedString::from)
        .collect::<Vec<_>>();
    ui.set_available_models(ModelRc::new(VecModel::from(models)));
}
