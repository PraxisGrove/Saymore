use std::collections::BTreeMap;

use super::{
    ActiveProviders, ChatCompletionsLlmSettings, ChatCompletionsProfile, LlmProviderPreset,
    MACOS_SPEECH_PROVIDER_ID, MACOS_SPEECH_PROVIDER_TYPE, OPENAI_TRANSCRIPTIONS_PROVIDER_TYPE,
    OpenAiCompatibleAsrSettings, ProviderCatalog, ProviderDataConsent, ProviderInstance,
    QWEN3_ASR_PROVIDER_ID, QWEN3_ASR_PROVIDER_TYPE, SENSE_VOICE_PROVIDER_ID,
    SENSE_VOICE_PROVIDER_TYPE, VOLCENGINE_PROVIDER_TYPE, VolcengineAsrSettings,
    WHISPER_PROVIDER_ID, WHISPER_PROVIDER_TYPE, provider_config,
};

#[test]
fn built_in_provider_configurations_remain_independent_when_switching() {
    let providers = [
        LlmProviderPreset::Qwen,
        LlmProviderPreset::VolcengineArk,
        LlmProviderPreset::OpenAi,
        LlmProviderPreset::Kimi,
        LlmProviderPreset::Gemini,
        LlmProviderPreset::OpenRouter,
        LlmProviderPreset::ZhipuGlm,
        LlmProviderPreset::MiniMax,
    ];
    let mut catalog = ProviderCatalog::default();

    for (index, preset) in providers.into_iter().enumerate() {
        let api_key = format!("key-{index}");
        let model = format!("model-{index}");
        catalog.save_llm_provider_model_config(preset, &api_key, &model);
        catalog.select_llm_provider(preset);

        assert_eq!(Some(preset), catalog.active_llm_provider());
        assert_eq!(Some(api_key.as_str()), catalog.llm_provider_api_key(preset));
        assert_eq!(
            Some(model.as_str()),
            catalog.configured_llm_provider_model(preset)
        );
        let Some(settings) = catalog.llm_provider_settings(preset) else {
            panic!("saved provider settings should remain available");
        };
        assert_eq!(preset.base_url(), settings.base_url);
        assert_eq!(preset.profile().chat_completions, settings.profile);
    }

    assert_eq!(providers.len(), catalog.llm_providers.len());
}

#[test]
fn qwen_preserves_a_workspace_specific_endpoint() {
    let mut catalog = ProviderCatalog::default();
    let endpoint = "https://workspace.cn-beijing.maas.aliyuncs.com/compatible-mode/v1";

    catalog.save_llm_provider_endpoint_config(
        LlmProviderPreset::Qwen,
        endpoint,
        "qwen-key",
        "qwen-plus",
    );

    let Some(settings) = catalog.llm_provider_settings(LlmProviderPreset::Qwen) else {
        panic!("saved Qwen settings should remain available");
    };
    assert_eq!(endpoint, settings.base_url);
    assert_eq!(ChatCompletionsProfile::Qwen, settings.profile);
}

#[test]
fn static_catalog_providers_expose_recommended_models_without_a_models_endpoint() {
    assert_eq!(
        ["qwen-plus", "qwen-flash"],
        LlmProviderPreset::Qwen.recommended_models()
    );
    assert_eq!(
        ["doubao-seed-2-0-lite-260215"],
        LlmProviderPreset::VolcengineArk.recommended_models()
    );
    assert_eq!(
        ["glm-4.7-flash", "glm-5.2"],
        LlmProviderPreset::ZhipuGlm.recommended_models()
    );
    for preset in [
        LlmProviderPreset::Qwen,
        LlmProviderPreset::VolcengineArk,
        LlmProviderPreset::ZhipuGlm,
    ] {
        assert!(!preset.supports_remote_model_discovery());
    }
}

#[test]
fn minimax_uses_the_official_openai_compatible_model_catalog() {
    let preset = LlmProviderPreset::MiniMax;

    assert_eq!("https://api.minimaxi.com/v1", preset.base_url());
    assert_eq!("MiniMax-M3", preset.model());
    assert_eq!(["MiniMax-M3", "MiniMax-M2.7"], preset.recommended_models());
    assert_eq!(
        "https://api.minimaxi.com/v1/models",
        preset.model_list_url()
    );
    assert!(preset.supports_remote_model_discovery());
    assert_eq!(
        ChatCompletionsProfile::MiniMax,
        preset.profile().chat_completions
    );
}

#[test]
fn legacy_deepseek_config_recovers_protocol_from_provider_identity() {
    let mut catalog = ProviderCatalog::default();
    catalog.llm_providers.push(ProviderInstance {
        id: "deepseek".to_owned(),
        name: "DeepSeek".to_owned(),
        provider_type: "openai_compatible".to_owned(),
        config: serde_json::json!({
            "base_url": "https://api.deepseek.com",
            "api_key": "saved",
            "model": "renamed-model",
            "custom_headers": {}
        }),
        data_consent: None,
    });

    let settings = catalog.llm_provider_settings(LlmProviderPreset::DeepSeek);

    assert_eq!(
        Some(ChatCompletionsProfile::DeepSeek),
        settings.map(|settings| settings.profile)
    );
}

#[test]
fn selecting_whisper_creates_one_stable_provider() {
    let mut catalog = ProviderCatalog::default();

    catalog.select_whisper_provider();
    catalog.select_whisper_provider();

    assert!(catalog.whisper_is_active());
    assert_eq!(Some(WHISPER_PROVIDER_ID), catalog.active.asr.as_deref());
    assert_eq!(
        1,
        catalog
            .asr_providers
            .iter()
            .filter(|provider| provider.provider_type == WHISPER_PROVIDER_TYPE)
            .count()
    );
}

#[test]
fn selecting_qwen3_asr_creates_one_stable_provider() {
    let mut catalog = ProviderCatalog::default();
    catalog.select_qwen3_asr_provider();
    catalog.select_qwen3_asr_provider();
    assert!(catalog.qwen3_asr_is_active());
    assert_eq!(Some(QWEN3_ASR_PROVIDER_ID), catalog.active.asr.as_deref());
    assert_eq!(
        1,
        catalog
            .asr_providers
            .iter()
            .filter(|provider| provider.provider_type == QWEN3_ASR_PROVIDER_TYPE)
            .count()
    );
}

#[test]
fn selecting_sense_voice_creates_one_stable_provider() {
    let mut catalog = ProviderCatalog::default();
    catalog.select_sense_voice_provider();
    catalog.select_sense_voice_provider();

    assert!(catalog.sense_voice_is_active());
    assert_eq!(Some(SENSE_VOICE_PROVIDER_ID), catalog.active.asr.as_deref());
    assert_eq!(
        1,
        catalog
            .asr_providers
            .iter()
            .filter(|provider| provider.provider_type == SENSE_VOICE_PROVIDER_TYPE)
            .count()
    );
    assert!(catalog.clear_sense_voice_selection());
    assert!(!catalog.sense_voice_is_active());
}

#[test]
fn exposes_a_provider_model_only_after_configuration_is_saved() {
    let mut catalog = ProviderCatalog::default();

    assert_eq!(
        None,
        catalog.configured_llm_provider_model(LlmProviderPreset::DeepSeek)
    );

    catalog.select_llm_provider(LlmProviderPreset::DeepSeek);
    assert_eq!(
        None,
        catalog.configured_llm_provider_model(LlmProviderPreset::DeepSeek)
    );

    catalog.save_llm_provider_config(LlmProviderPreset::DeepSeek, "saved-key");
    assert_eq!(
        Some("deepseek-v4-flash"),
        catalog.configured_llm_provider_model(LlmProviderPreset::DeepSeek)
    );
}

#[test]
fn saves_the_model_selected_by_the_user() {
    let mut catalog = ProviderCatalog::default();

    catalog.save_llm_provider_model_config(
        LlmProviderPreset::DeepSeek,
        "saved-key",
        "deepseek-v4-pro",
    );

    assert_eq!(
        Some("deepseek-v4-pro"),
        catalog.configured_llm_provider_model(LlmProviderPreset::DeepSeek)
    );
}

#[test]
fn changing_credentials_or_model_preserves_endpoint_consent() {
    let mut catalog = ProviderCatalog::default();
    catalog.save_llm_provider_config(LlmProviderPreset::DeepSeek, "old-key");
    catalog.llm_providers[0].data_consent = Some(ProviderDataConsent {
        fingerprint: "endpoint:https://api.deepseek.com".to_owned(),
    });

    catalog.save_llm_provider_model_config(
        LlmProviderPreset::DeepSeek,
        "new-key",
        "deepseek-v4-pro",
    );

    assert_eq!(
        Some("endpoint:https://api.deepseek.com"),
        catalog.llm_providers[0]
            .data_consent
            .as_ref()
            .map(|consent| consent.fingerprint.as_str())
    );
}

#[test]
fn custom_provider_round_trips_user_owned_connection_settings() {
    let mut catalog = ProviderCatalog::default();

    catalog.save_custom_llm_provider_config(" http://localhost:11434/v1/ ", "", "qwen3:8b");
    catalog.select_llm_provider(LlmProviderPreset::Custom);

    assert_eq!(
        Some(LlmProviderPreset::Custom),
        catalog.active_llm_provider()
    );
    assert_eq!(
        Some("qwen3:8b"),
        catalog.configured_llm_provider_model(LlmProviderPreset::Custom)
    );
    let Some(settings) = catalog.llm_provider_settings(LlmProviderPreset::Custom) else {
        panic!("custom provider settings should be available");
    };
    assert_eq!("http://localhost:11434/v1", settings.base_url);
    assert_eq!("", settings.api_key);
    assert_eq!("qwen3:8b", settings.model);
}

#[test]
fn active_generic_compatible_provider_is_adopted_as_custom() {
    let mut catalog = ProviderCatalog {
        active: ActiveProviders {
            asr: None,
            llm: Some("legacy-custom".to_owned()),
        },
        asr_providers: Vec::new(),
        llm_providers: vec![ProviderInstance {
            id: "legacy-custom".to_owned(),
            name: "Local model".to_owned(),
            provider_type: "openai_compatible".to_owned(),
            config: provider_config(&ChatCompletionsLlmSettings {
                base_url: "http://localhost:11434/v1".to_owned(),
                api_key: String::new(),
                model: "qwen3:8b".to_owned(),
                custom_headers: BTreeMap::new(),
                profile: ChatCompletionsProfile::Portable,
            }),
            data_consent: None,
        }],
    };

    assert_eq!(
        Some(LlmProviderPreset::Custom),
        catalog.active_llm_provider()
    );
    catalog.save_custom_llm_provider_config("http://localhost:11434/v1", "", "qwen3:8b");
    assert_eq!(Some("custom"), catalog.active.llm.as_deref());
    assert_eq!("custom", catalog.llm_providers[0].id);
}

#[test]
fn selecting_macos_speech_preserves_other_asr_providers() {
    let mut catalog = ProviderCatalog {
        active: ActiveProviders {
            asr: Some("volcengine-one".to_owned()),
            llm: None,
        },
        asr_providers: vec![ProviderInstance {
            id: "volcengine-one".to_owned(),
            name: "Volcengine".to_owned(),
            provider_type: "volcengine".to_owned(),
            config: serde_json::json!({"api_key": "saved", "model": "model"}),
            data_consent: None,
        }],
        llm_providers: Vec::new(),
    };

    catalog.select_macos_speech_provider();

    assert!(catalog.macos_speech_is_active());
    assert_eq!(2, catalog.asr_providers.len());
    assert_eq!(
        Some("saved"),
        catalog.asr_providers[0]
            .config
            .get("api_key")
            .and_then(serde_json::Value::as_str)
    );
}

#[test]
fn selecting_macos_speech_reuses_its_stable_provider() {
    let mut catalog = ProviderCatalog::default();

    catalog.select_macos_speech_provider();
    catalog.select_macos_speech_provider();

    assert!(catalog.macos_speech_is_active());
    assert_eq!(1, catalog.asr_providers.len());
}

#[test]
fn selecting_a_configured_cloud_asr_provider_updates_only_the_active_provider() {
    let mut catalog = ProviderCatalog {
        active: ActiveProviders {
            asr: Some(MACOS_SPEECH_PROVIDER_ID.to_owned()),
            llm: None,
        },
        asr_providers: vec![
            ProviderInstance {
                id: MACOS_SPEECH_PROVIDER_ID.to_owned(),
                name: "macOS Dictation".to_owned(),
                provider_type: MACOS_SPEECH_PROVIDER_TYPE.to_owned(),
                config: serde_json::json!({}),
                data_consent: None,
            },
            ProviderInstance {
                id: "volcengine-one".to_owned(),
                name: "Volcengine".to_owned(),
                provider_type: VOLCENGINE_PROVIDER_TYPE.to_owned(),
                config: serde_json::json!({"api_key": "saved", "model": "model"}),
                data_consent: None,
            },
            ProviderInstance {
                id: "custom-one".to_owned(),
                name: "Custom".to_owned(),
                provider_type: OPENAI_TRANSCRIPTIONS_PROVIDER_TYPE.to_owned(),
                config: serde_json::json!({"api_key": "saved", "model": "model"}),
                data_consent: None,
            },
        ],
        llm_providers: Vec::new(),
    };

    assert!(catalog.select_volcengine_asr_provider());
    assert_eq!(Some("volcengine-one"), catalog.active.asr.as_deref());
    assert_eq!(3, catalog.asr_providers.len());

    assert!(catalog.select_openai_transcriptions_asr_provider());
    assert_eq!(Some("custom-one"), catalog.active.asr.as_deref());
    assert_eq!(3, catalog.asr_providers.len());
}

#[test]
fn selecting_an_unconfigured_cloud_asr_provider_keeps_the_current_provider() {
    let mut catalog = ProviderCatalog {
        active: ActiveProviders {
            asr: Some(MACOS_SPEECH_PROVIDER_ID.to_owned()),
            llm: None,
        },
        asr_providers: vec![ProviderInstance {
            id: MACOS_SPEECH_PROVIDER_ID.to_owned(),
            name: "macOS Dictation".to_owned(),
            provider_type: MACOS_SPEECH_PROVIDER_TYPE.to_owned(),
            config: serde_json::json!({}),
            data_consent: None,
        }],
        llm_providers: Vec::new(),
    };

    assert!(!catalog.select_volcengine_asr_provider());
    assert_eq!(
        Some(MACOS_SPEECH_PROVIDER_ID),
        catalog.active.asr.as_deref()
    );
}

#[test]
fn configuring_cloud_asr_providers_preserves_the_current_provider() {
    let mut catalog = ProviderCatalog::default();
    catalog.select_macos_speech_provider();

    catalog.configure_volcengine_asr_provider(&VolcengineAsrSettings {
        enabled: true,
        api_key: "volc-key".to_owned(),
        model: "volc-model".to_owned(),
    });
    catalog.configure_openai_transcriptions_asr_provider(&OpenAiCompatibleAsrSettings {
        enabled: true,
        base_url: "https://example.com/v1".to_owned(),
        api_key: "custom-key".to_owned(),
        model: "custom-model".to_owned(),
    });

    assert!(catalog.macos_speech_is_active());
    assert_eq!(3, catalog.asr_providers.len());
    assert_eq!(
        Some("volc-key"),
        catalog
            .asr_providers
            .iter()
            .find(|provider| provider.provider_type == VOLCENGINE_PROVIDER_TYPE)
            .and_then(|provider| provider.config.get("api_key"))
            .and_then(serde_json::Value::as_str)
    );
    assert_eq!(
        Some("custom-model"),
        catalog
            .asr_providers
            .iter()
            .find(|provider| provider.provider_type == OPENAI_TRANSCRIPTIONS_PROVIDER_TYPE)
            .and_then(|provider| provider.config.get("model"))
            .and_then(serde_json::Value::as_str)
    );
}
