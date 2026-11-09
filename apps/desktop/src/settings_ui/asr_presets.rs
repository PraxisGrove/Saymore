use slint::{ComponentHandle, SharedString};
use template_app::CloudAsrPreset;

use crate::ui::{
    AppWindow, AsrConfigurationField, AsrProvider, AsrProviderCardKind, CloudAsrService,
};

pub(super) fn wire(ui: &AppWindow) {
    let weak_ui = ui.as_weak();
    ui.on_prepare_cloud_asr(move |service| {
        let Some(ui) = weak_ui.upgrade() else {
            return;
        };
        if ui.get_asr_testing() {
            return;
        }
        let preset = preset(service);
        ui.set_custom_asr_service_name(preset.display_name().into());
        ui.set_draft_custom_asr_kind(configured_card_kind(preset.base_url()));
        let draft = PresetDraft::new(
            preset,
            &ui.get_custom_asr_base_url(),
            &ui.get_custom_asr_api_key(),
            &ui.get_custom_asr_model(),
        );
        ui.invoke_invalidate_model_discovery();
        ui.set_asr_provider(AsrProvider::Custom);
        ui.set_custom_asr_base_url(draft.base_url.into());
        ui.set_custom_asr_api_key(draft.api_key.into());
        ui.set_custom_asr_model(draft.model.into());
        ui.set_asr_config_dirty(true);
        ui.set_asr_draft_error(false);
        ui.set_asr_error_field(AsrConfigurationField::None);
        ui.set_asr_config_status(SharedString::default());
        ui.set_asr_pending_test(false);
        ui.set_asr_test_succeeded(false);
        ui.set_asr_test_elapsed(SharedString::default());
        ui.set_asr_test_result(SharedString::default());
    });
}

pub(super) fn configured_card_kind(base_url: &str) -> AsrProviderCardKind {
    match CloudAsrPreset::from_base_url(base_url) {
        Some(CloudAsrPreset::OpenAi) => AsrProviderCardKind::Openai,
        Some(CloudAsrPreset::Groq) => AsrProviderCardKind::Groq,
        Some(CloudAsrPreset::Mistral) => AsrProviderCardKind::Mistral,
        None => AsrProviderCardKind::Custom,
    }
}

fn preset(service: CloudAsrService) -> CloudAsrPreset {
    match service {
        CloudAsrService::Openai => CloudAsrPreset::OpenAi,
        CloudAsrService::Groq => CloudAsrPreset::Groq,
        CloudAsrService::Mistral => CloudAsrPreset::Mistral,
    }
}

#[derive(Debug, PartialEq, Eq)]
struct PresetDraft {
    base_url: String,
    api_key: String,
    model: String,
}

impl PresetDraft {
    fn new(preset: CloudAsrPreset, current_url: &str, api_key: &str, model: &str) -> Self {
        let same_service = CloudAsrPreset::from_base_url(current_url) == Some(preset);
        Self {
            base_url: preset.base_url().to_owned(),
            api_key: if same_service {
                api_key.to_owned()
            } else {
                String::new()
            },
            model: if same_service && !model.trim().is_empty() {
                model.to_owned()
            } else {
                preset.default_model().to_owned()
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_service_matches_the_saved_endpoint() {
        for (url, expected) in [
            (
                CloudAsrPreset::OpenAi.base_url(),
                AsrProviderCardKind::Openai,
            ),
            (CloudAsrPreset::Groq.base_url(), AsrProviderCardKind::Groq),
            (
                CloudAsrPreset::Mistral.base_url(),
                AsrProviderCardKind::Mistral,
            ),
            ("https://example.com/v1", AsrProviderCardKind::Custom),
        ] {
            assert_eq!(expected, configured_card_kind(url));
        }
    }

    #[test]
    fn switching_services_clears_credentials_and_uses_the_new_model() {
        let draft = PresetDraft::new(
            CloudAsrPreset::Groq,
            CloudAsrPreset::OpenAi.base_url(),
            "old-provider-key",
            "old-provider-model",
        );
        assert_eq!(
            PresetDraft {
                base_url: CloudAsrPreset::Groq.base_url().to_owned(),
                api_key: String::new(),
                model: CloudAsrPreset::Groq.default_model().to_owned(),
            },
            draft
        );
    }

    #[test]
    fn opening_the_same_service_keeps_the_users_key_and_model() {
        let draft = PresetDraft::new(
            CloudAsrPreset::Groq,
            " https://api.groq.com/openai/v1/audio/transcriptions/ ",
            "saved-key",
            "whisper-large-v3",
        );
        assert_eq!("saved-key", draft.api_key);
        assert_eq!("whisper-large-v3", draft.model);
    }

    #[test]
    fn unknown_compatible_endpoints_do_not_share_keys_with_presets() {
        let draft = PresetDraft::new(
            CloudAsrPreset::OpenAi,
            "https://example.com/v1",
            "custom-key",
            "custom-model",
        );
        assert!(draft.api_key.is_empty());
        assert_eq!(CloudAsrPreset::OpenAi.default_model(), draft.model);
    }
}
