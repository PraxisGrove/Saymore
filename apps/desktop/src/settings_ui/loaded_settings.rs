use slint::{ComponentHandle, SharedString};
use template_app::{
    LlmProviderPreset, ProviderCatalog, ProviderConfigStore, SaymoreSettings, SettingsStore,
};
use template_infra::JsonSettingsStore;

use crate::ui::{AppWindow, AsrProvider as UiAsrProvider, Translations};

use super::{
    VOLCENGINE_ASR_2_MODEL, apply_pending_test, apply_status, llm_configuration_ready,
    provider_is_local, ui_provider, volcengine_api_key_is_valid, volcengine_model_id,
};

pub(super) fn apply_loaded_settings(ui: &AppWindow, store: &JsonSettingsStore) {
    match (store.load(), store.load_catalog()) {
        (Ok(settings), Ok(catalog)) => {
            apply_loaded_llm(ui, &settings, &catalog);
            let configured = apply_loaded_asr(ui, settings);
            ui.set_asr_testing(false);
            apply_pending_test(ui, configured);
            ui.set_asr_config_dirty(false);
            ui.set_llm_config_dirty(false);
        }
        _ => apply_status(
            ui,
            false,
            true,
            ui.global::<Translations>()
                .get_common_configuration_load_failed(),
        ),
    }
}

fn apply_loaded_llm(ui: &AppWindow, settings: &SaymoreSettings, catalog: &ProviderCatalog) {
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
    let sensenova_model = catalog.configured_llm_provider_model(LlmProviderPreset::SenseNova);
    let deepseek_model = catalog.configured_llm_provider_model(LlmProviderPreset::DeepSeek);
    let custom_settings = catalog
        .llm_provider_settings(LlmProviderPreset::Custom)
        .unwrap_or_default();
    let custom_model = catalog.configured_llm_provider_model(LlmProviderPreset::Custom);
    ui.set_sensenova_configured(sensenova_model.is_some());
    ui.set_deepseek_configured(deepseek_model.is_some());
    ui.set_sensenova_model(SharedString::from(
        sensenova_model.unwrap_or_else(|| LlmProviderPreset::SenseNova.model()),
    ));
    ui.set_deepseek_model(SharedString::from(
        deepseek_model.unwrap_or_else(|| LlmProviderPreset::DeepSeek.model()),
    ));
    ui.set_custom_llm_api_key(SharedString::from(&custom_settings.api_key));
    ui.set_custom_llm_base_url(SharedString::from(&custom_settings.base_url));
    ui.set_custom_llm_model(SharedString::from(&custom_settings.model));
    ui.set_custom_llm_configured(custom_model.is_some());
    let selected_settings = catalog
        .llm_provider_settings(selected)
        .unwrap_or_else(|| selected.settings(""));
    let llm_configured = llm_configuration_ready(selected, &selected_settings);
    let llm_base_url = selected_settings.base_url;
    let llm_enabled = settings.llm.enabled
        && llm_configured
        && settings.llm.confirmed_base_url.trim() == llm_base_url;
    ui.set_llm_enabled(llm_enabled);
    ui.set_llm_provider_target(SharedString::from(&llm_base_url));
    ui.set_llm_provider_local(provider_is_local(&llm_base_url));
    let translations = ui.global::<Translations>();
    ui.set_llm_config_status(if llm_enabled {
        translations.get_models_enabled()
    } else if llm_configured {
        translations.get_models_not_enabled()
    } else {
        translations.get_models_save_current_provider_key()
    });
}

fn apply_loaded_asr(ui: &AppWindow, settings: SaymoreSettings) -> bool {
    let volcengine = settings.asr.volcengine;
    let custom = settings.asr.openai_compatible;
    let volcengine_api_key = volcengine.api_key.trim();
    let invalid_api_key =
        !volcengine_api_key.is_empty() && !volcengine_api_key_is_valid(volcengine_api_key);
    let volcengine_configured = !volcengine_api_key.is_empty()
        && !invalid_api_key
        && volcengine_model_id(&volcengine.model).is_ok();
    let custom_configured = !custom.api_key.trim().is_empty()
        && !custom.base_url.trim().is_empty()
        && !custom.model.trim().is_empty();
    let custom_active = custom.enabled;
    let configured = if custom_active {
        custom_configured
    } else {
        volcengine.enabled && volcengine_configured
    };
    ui.set_asr_provider(if custom_active {
        UiAsrProvider::Custom
    } else {
        UiAsrProvider::Volcengine
    });
    ui.set_asr_api_key(SharedString::from(volcengine.api_key));
    ui.set_asr_model(SharedString::from(if volcengine.model.trim().is_empty() {
        VOLCENGINE_ASR_2_MODEL
    } else {
        volcengine.model.as_str()
    }));
    ui.set_custom_asr_api_key(SharedString::from(custom.api_key));
    ui.set_custom_asr_base_url(SharedString::from(custom.base_url));
    ui.set_custom_asr_model(SharedString::from(custom.model));
    ui.set_custom_asr_configured(custom_configured);
    let translations = ui.global::<Translations>();
    apply_status(
        ui,
        configured,
        !custom_active && invalid_api_key,
        if !custom_active && invalid_api_key {
            translations.get_models_invalid_api_key()
        } else if configured {
            translations.get_models_configured()
        } else {
            translations.get_models_not_configured()
        },
    );
    configured
}
