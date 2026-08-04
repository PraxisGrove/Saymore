use slint::{ComponentHandle, SharedString};
use template_app::{
    LlmProviderPreset, ProviderCatalog, ProviderConfigStore, SaymoreSettings, SettingsStore,
    vocabulary_suggestion_consent_fingerprint,
};
use template_infra::JsonSettingsStore;

use crate::ui::{
    AppWindow, AsrConfigurationField as UiAsrConfigurationField, AsrProvider as UiAsrProvider,
    LlmProviderConfigurationState, Translations,
};

use super::{
    VOLCENGINE_ASR_2_MODEL, apply_pending_test, apply_status, llm_configuration_ready,
    model_discovery::restore_llm_model_catalog, provider_is_local, ui_provider,
    volcengine_api_key_is_valid, volcengine_model_id,
};

pub(super) fn apply_loaded_settings(ui: &AppWindow, store: &JsonSettingsStore) {
    match (store.load(), store.load_catalog()) {
        (Ok(settings), Ok(catalog)) => {
            apply_loaded_llm(ui, &settings, &catalog);
            let configured = apply_loaded_asr(ui, settings, &catalog);
            ui.invoke_asr_provider_selection_applied();
            ui.set_asr_testing(false);
            ui.set_asr_test_succeeded(false);
            ui.set_asr_test_elapsed(SharedString::default());
            ui.set_asr_test_result(SharedString::default());
            apply_pending_test(ui, configured);
            ui.set_asr_config_dirty(false);
            ui.set_asr_draft_error(false);
            ui.set_asr_error_field(UiAsrConfigurationField::None);
            ui.set_llm_config_dirty(false);
            ui.set_llm_draft_error(false);
            ui.set_llm_testing(false);
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
    let selected = selected_llm_provider(catalog);
    ui.set_llm_provider(ui_provider(selected));
    ui.set_active_llm_provider(ui_provider(selected));
    ui.set_llm_provider_configured(provider_configuration_state(catalog));
    apply_llm_draft(ui, catalog, selected);
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
    let assist_provider_id = catalog
        .active
        .llm
        .as_deref()
        .unwrap_or_else(|| selected.id());
    let assist_configured = catalog.active_llm_provider().is_some() && llm_configured;
    ui.set_dictionary_assist_provider_configured(assist_configured);
    ui.set_dictionary_assist_provider_target(SharedString::from(&llm_base_url));
    ui.set_dictionary_assist_consent_fingerprint(SharedString::from(if assist_configured {
        vocabulary_suggestion_consent_fingerprint(assist_provider_id, &llm_base_url)
    } else {
        String::new()
    }));
    let translations = ui.global::<Translations>();
    ui.set_llm_config_status(if llm_enabled {
        translations.get_models_enabled()
    } else if llm_configured {
        translations.get_models_not_enabled()
    } else {
        translations.get_models_save_current_provider_key()
    });
}

pub(super) fn apply_llm_draft(
    ui: &AppWindow,
    catalog: &ProviderCatalog,
    provider: LlmProviderPreset,
) {
    let settings = catalog
        .llm_provider_settings(provider)
        .unwrap_or_else(|| provider.settings(""));
    ui.set_llm_saved_api_key(SharedString::from(&settings.api_key));
    ui.set_llm_saved_base_url(SharedString::from(&settings.base_url));
    ui.set_llm_saved_model(SharedString::from(&settings.model));
    ui.set_llm_draft_api_key(SharedString::from(settings.api_key));
    ui.set_llm_draft_base_url(SharedString::from(settings.base_url));
    ui.set_llm_draft_model(SharedString::from(settings.model));
    restore_llm_model_catalog(ui, catalog, provider);
    ui.set_llm_config_dirty(
        ui.get_llm_draft_api_key() != ui.get_llm_saved_api_key()
            || ui.get_llm_draft_base_url() != ui.get_llm_saved_base_url()
            || ui.get_llm_draft_model() != ui.get_llm_saved_model(),
    );
    ui.set_llm_draft_error(false);
}

fn provider_configuration_state(catalog: &ProviderCatalog) -> LlmProviderConfigurationState {
    let configured = |provider| catalog.configured_llm_provider_model(provider).is_some();
    LlmProviderConfigurationState {
        sensenova: configured(LlmProviderPreset::SenseNova),
        deepseek: configured(LlmProviderPreset::DeepSeek),
        qwen: configured(LlmProviderPreset::Qwen),
        volcengine_ark: configured(LlmProviderPreset::VolcengineArk),
        openai: configured(LlmProviderPreset::OpenAi),
        kimi: configured(LlmProviderPreset::Kimi),
        gemini: configured(LlmProviderPreset::Gemini),
        openrouter: configured(LlmProviderPreset::OpenRouter),
        zhipu_glm: configured(LlmProviderPreset::ZhipuGlm),
        minimax: configured(LlmProviderPreset::MiniMax),
        siliconflow: configured(LlmProviderPreset::SiliconFlow),
        stepfun: configured(LlmProviderPreset::StepFun),
        custom: configured(LlmProviderPreset::Custom),
    }
}

fn selected_llm_provider(catalog: &ProviderCatalog) -> LlmProviderPreset {
    catalog
        .active_llm_provider()
        .or_else(|| {
            LlmProviderPreset::ALL
                .into_iter()
                .find(|provider| catalog.configured_llm_provider_model(*provider).is_some())
        })
        .unwrap_or(LlmProviderPreset::SenseNova)
}

fn apply_loaded_asr(ui: &AppWindow, settings: SaymoreSettings, catalog: &ProviderCatalog) -> bool {
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
    let paraformer_active = catalog.paraformer_is_active();
    let whisper_active = catalog.whisper_is_active();
    let qwen3_active = catalog.qwen3_asr_is_active();
    let sense_voice_active = catalog.sense_voice_is_active();
    ui.set_paraformer_selected(paraformer_active);
    ui.set_whisper_selected(whisper_active);
    ui.set_qwen3_asr_selected(qwen3_active);
    ui.set_sense_voice_selected(sense_voice_active);
    let macos_speech_active = catalog.macos_speech_is_active();
    let macos_speech_ready = super::apply_macos_speech_state(ui, macos_speech_active);
    let configured = if sense_voice_active || qwen3_active || whisper_active || paraformer_active {
        true
    } else if macos_speech_active {
        macos_speech_ready
    } else if custom_active {
        custom_configured
    } else {
        volcengine.enabled && volcengine_configured
    };
    let selected_cloud_provider = selected_cloud_asr_provider(volcengine.enabled, custom_active);
    let active_cloud_provider = selected_cloud_provider.unwrap_or(UiAsrProvider::Volcengine);
    ui.set_cloud_asr_selected(selected_cloud_provider.is_some());
    ui.set_asr_provider(active_cloud_provider);
    ui.set_active_asr_provider(active_cloud_provider);
    ui.set_asr_api_key(SharedString::from(volcengine.api_key));
    ui.set_volcengine_asr_configured(volcengine_configured);
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
        !whisper_active
            && !paraformer_active
            && !macos_speech_active
            && !custom_active
            && invalid_api_key,
        if !whisper_active
            && !paraformer_active
            && !macos_speech_active
            && !custom_active
            && invalid_api_key
        {
            translations.get_models_invalid_api_key()
        } else if configured {
            translations.get_models_configured()
        } else {
            translations.get_models_not_configured()
        },
    );
    configured
}

fn selected_cloud_asr_provider(
    volcengine_enabled: bool,
    custom_enabled: bool,
) -> Option<UiAsrProvider> {
    if custom_enabled {
        Some(UiAsrProvider::Custom)
    } else if volcengine_enabled {
        Some(UiAsrProvider::Volcengine)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restores_a_configured_provider_when_legacy_disabled_state_has_no_active_provider() {
        let mut catalog = ProviderCatalog::default();
        catalog.save_llm_provider_config(LlmProviderPreset::DeepSeek, "deepseek-key");

        assert_eq!(LlmProviderPreset::DeepSeek, selected_llm_provider(&catalog));
    }

    #[test]
    fn keeps_the_active_provider_when_multiple_providers_are_configured() {
        let mut catalog = ProviderCatalog::default();
        catalog.save_llm_provider_config(LlmProviderPreset::SenseNova, "sense-key");
        catalog.save_llm_provider_config(LlmProviderPreset::DeepSeek, "deepseek-key");
        catalog.select_llm_provider(LlmProviderPreset::DeepSeek);

        assert_eq!(LlmProviderPreset::DeepSeek, selected_llm_provider(&catalog));
    }

    #[test]
    fn configured_but_inactive_cloud_asr_is_not_selected() {
        assert_eq!(None, selected_cloud_asr_provider(false, false));
        assert_eq!(
            Some(UiAsrProvider::Volcengine),
            selected_cloud_asr_provider(true, false)
        );
        assert_eq!(
            Some(UiAsrProvider::Custom),
            selected_cloud_asr_provider(false, true)
        );
    }
}
