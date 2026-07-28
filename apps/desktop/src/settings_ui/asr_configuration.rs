use std::sync::Arc;

use slint::{ComponentHandle, SharedString};
use template_app::{
    OpenAiCompatibleAsrSettings, ProviderConfigStore, SettingsStore, SettingsStoreError,
    SpeechRecognitionError, VolcengineAsrSettings,
};
use template_infra::{JsonSettingsStore, OpenAiCompatibleSpeechRecognizer};
use uuid::Uuid;

use crate::ui::{
    AppWindow, AsrConfigurationField as UiAsrConfigurationField, AsrProvider as UiAsrProvider,
    Translations,
};

use super::{
    VOLCENGINE_ASR_1_MODEL, VOLCENGINE_ASR_2_MODEL, VOLCENGINE_LEGACY_MODEL, apply_loaded_settings,
    apply_pending_test, apply_status,
};

mod recognition_test;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AsrConfigError {
    MissingApiKey,
    InvalidApiKey,
    MissingBaseUrl,
    InvalidBaseUrl,
    MissingModel,
    UnsupportedModel,
    Store,
}

#[derive(Clone)]
enum AsrCandidate {
    Volcengine(VolcengineAsrSettings),
    Custom(OpenAiCompatibleAsrSettings),
}

#[derive(Debug)]
enum AsrSaveError {
    Configuration(AsrConfigError),
    Connection(SpeechRecognitionError),
}

#[derive(Clone, Copy)]
enum TestPurpose {
    Save,
    TestOnly,
}

pub(super) fn wire(ui: &AppWindow, store: Arc<JsonSettingsStore>) {
    let save_ui = ui.as_weak();
    let save_store = Arc::clone(&store);
    ui.on_save_asr_config(move |provider, api_key, base_url, model| {
        let Some(ui) = save_ui.upgrade() else {
            return;
        };
        if ui.get_asr_testing() {
            return;
        }
        begin_connection_test(
            &ui,
            Arc::clone(&save_store),
            candidate(provider, &api_key, &base_url, &model),
            TestPurpose::Save,
        );
    });

    wire_provider_selection(ui, Arc::clone(&store));

    let test_ui = ui.as_weak();
    let test_store = Arc::clone(&store);
    ui.on_request_asr_test(move || {
        let Some(ui) = test_ui.upgrade() else {
            return;
        };
        if ui.get_asr_testing() {
            return;
        }
        let candidate = current_candidate(&ui);
        begin_connection_test(
            &ui,
            Arc::clone(&test_store),
            candidate,
            TestPurpose::TestOnly,
        );
    });

    let cancel_ui = ui.as_weak();
    let cancel_store = Arc::clone(&store);
    ui.on_cancel_asr_config(move || {
        if let Some(ui) = cancel_ui.upgrade() {
            apply_loaded_settings(&ui, &cancel_store);
        }
    });

    let delete_ui = ui.as_weak();
    ui.on_delete_asr_config(move || {
        let Some(ui) = delete_ui.upgrade() else {
            return;
        };
        match clear_asr_configuration(&store) {
            Ok(()) => {
                apply_loaded_settings(&ui, &store);
            }
            Err(_) => apply_status(
                &ui,
                true,
                true,
                ui.global::<Translations>().get_common_delete_failed(),
            ),
        }
    });
}

fn wire_provider_selection(ui: &AppWindow, store: Arc<JsonSettingsStore>) {
    let select_ui = ui.as_weak();
    let select_store = Arc::clone(&store);
    ui.on_select_asr_provider(move |provider| {
        if let Some(ui) = select_ui.upgrade() {
            apply_provider_status(&ui, &select_store, provider);
        }
    });

    let activate_ui = ui.as_weak();
    ui.on_activate_asr_provider(move |provider| {
        let Some(ui) = activate_ui.upgrade() else {
            return;
        };
        match select_configured_asr_provider(&store, provider) {
            Ok(true) => apply_loaded_settings(&ui, &store),
            Ok(false) => apply_provider_status(&ui, &store, provider),
            Err(_) => apply_status(
                &ui,
                false,
                true,
                ui.global::<Translations>().get_common_save_failed(),
            ),
        }
    });
}

fn current_candidate(ui: &AppWindow) -> Result<AsrCandidate, AsrConfigError> {
    match ui.get_asr_provider() {
        UiAsrProvider::Volcengine => candidate(
            UiAsrProvider::Volcengine,
            &ui.get_asr_api_key(),
            "",
            &ui.get_asr_model(),
        ),
        UiAsrProvider::Custom => candidate(
            UiAsrProvider::Custom,
            &ui.get_custom_asr_api_key(),
            &ui.get_custom_asr_base_url(),
            &ui.get_custom_asr_model(),
        ),
    }
}

fn candidate(
    provider: UiAsrProvider,
    api_key: &str,
    base_url: &str,
    model: &str,
) -> Result<AsrCandidate, AsrConfigError> {
    match provider {
        UiAsrProvider::Volcengine => {
            let api_key = api_key.trim();
            let model = volcengine_model_id(model)?;
            if api_key.is_empty() {
                return Err(AsrConfigError::MissingApiKey);
            }
            if !volcengine_api_key_is_valid(api_key) {
                return Err(AsrConfigError::InvalidApiKey);
            }
            Ok(AsrCandidate::Volcengine(VolcengineAsrSettings {
                enabled: true,
                api_key: api_key.to_owned(),
                model: model.to_owned(),
            }))
        }
        UiAsrProvider::Custom => {
            let api_key = api_key.trim();
            let base_url = base_url.trim().trim_end_matches('/');
            let model = model.trim();
            if api_key.is_empty() {
                return Err(AsrConfigError::MissingApiKey);
            }
            if base_url.is_empty() {
                return Err(AsrConfigError::MissingBaseUrl);
            }
            if model.is_empty() {
                return Err(AsrConfigError::MissingModel);
            }
            let settings = OpenAiCompatibleAsrSettings {
                enabled: true,
                base_url: base_url.to_owned(),
                api_key: api_key.to_owned(),
                model: model.to_owned(),
            };
            OpenAiCompatibleSpeechRecognizer::new(settings.clone())
                .map_err(|_| AsrConfigError::InvalidBaseUrl)?;
            Ok(AsrCandidate::Custom(settings))
        }
    }
}

fn begin_connection_test(
    ui: &AppWindow,
    store: Arc<JsonSettingsStore>,
    candidate: Result<AsrCandidate, AsrConfigError>,
    purpose: TestPurpose,
) {
    let candidate = match candidate {
        Ok(candidate) => candidate,
        Err(error) => {
            ui.set_asr_draft_error(true);
            ui.set_asr_error_field(config_error_field(error));
            ui.set_asr_config_status(asr_config_error_text(ui, error));
            return;
        }
    };
    ui.set_asr_testing(true);
    ui.set_asr_test_succeeded(false);
    ui.set_asr_test_elapsed(SharedString::default());
    ui.set_asr_draft_error(false);
    ui.set_asr_error_field(UiAsrConfigurationField::None);
    ui.set_asr_config_status(ui.global::<Translations>().get_models_testing_connection());
    let result_ui = ui.as_weak();
    let spawn_result = std::thread::Builder::new()
        .name("saymore-test-asr".to_owned())
        .spawn(move || {
            let attempt = recognition_test::run(&candidate);
            let result = attempt
                .result
                .map_err(AsrSaveError::Connection)
                .and_then(|()| match purpose {
                    TestPurpose::Save => {
                        save_candidate(&store, &candidate).map_err(AsrSaveError::Configuration)
                    }
                    TestPurpose::TestOnly => Ok(()),
                });
            if let Err(error) = &result {
                tracing::warn!(
                    target: "saymore::diagnostics",
                    event = "asr.connection_test_failed",
                    reason = ?error
                );
            }
            let _ = result_ui.upgrade_in_event_loop(move |ui| {
                finish_connection_test(&ui, &store, purpose, attempt.elapsed, result);
            });
        });
    if spawn_result.is_err() {
        tracing::warn!(
            target: "saymore::diagnostics",
            event = "asr.connection_test_worker_start_failed"
        );
        ui.set_asr_testing(false);
        ui.set_asr_test_succeeded(false);
        ui.set_asr_draft_error(true);
        ui.set_asr_error_field(UiAsrConfigurationField::Form);
        ui.set_asr_config_status(ui.global::<Translations>().get_models_connection_failed());
    }
}

fn finish_connection_test(
    ui: &AppWindow,
    store: &JsonSettingsStore,
    purpose: TestPurpose,
    elapsed: std::time::Duration,
    result: Result<(), AsrSaveError>,
) {
    ui.set_asr_testing(false);
    ui.set_asr_test_elapsed(format!("{:.2}", elapsed.as_secs_f64()).into());
    match result {
        Ok(()) => {
            let event = match purpose {
                TestPurpose::Save => "asr.configuration_saved",
                TestPurpose::TestOnly => "asr.connection_test_succeeded",
            };
            tracing::info!(target: "saymore::diagnostics", event = %event);
            ui.set_asr_draft_error(false);
            ui.set_asr_test_succeeded(true);
            ui.set_asr_error_field(UiAsrConfigurationField::None);
            match purpose {
                TestPurpose::Save => apply_loaded_settings(ui, store),
                TestPurpose::TestOnly => {}
            }
            apply_pending_test(ui, false);
            apply_status(
                ui,
                true,
                false,
                ui.global::<Translations>().get_models_connected(),
            );
        }
        Err(AsrSaveError::Configuration(error)) => {
            ui.set_asr_test_succeeded(false);
            ui.set_asr_draft_error(true);
            ui.set_asr_error_field(config_error_field(error));
            ui.set_asr_config_status(asr_config_error_text(ui, error));
        }
        Err(AsrSaveError::Connection(error)) => {
            ui.set_asr_test_succeeded(false);
            ui.set_asr_draft_error(true);
            ui.set_asr_error_field(UiAsrConfigurationField::Form);
            ui.set_asr_config_status(asr_test_failure_status(ui, &error));
        }
    }
}

#[cfg(test)]
fn test_and_save(
    store: &JsonSettingsStore,
    candidate: &AsrCandidate,
    test: impl FnOnce(&AsrCandidate) -> Result<(), SpeechRecognitionError>,
) -> Result<(), AsrSaveError> {
    test(candidate).map_err(AsrSaveError::Connection)?;
    save_candidate(store, candidate).map_err(AsrSaveError::Configuration)
}

fn save_candidate(
    store: &JsonSettingsStore,
    candidate: &AsrCandidate,
) -> Result<(), AsrConfigError> {
    store
        .load()
        .and_then(|mut settings| {
            match candidate {
                AsrCandidate::Volcengine(candidate) => {
                    settings.asr.volcengine = candidate.clone();
                    settings.asr.openai_compatible.enabled = false;
                }
                AsrCandidate::Custom(candidate) => {
                    settings.asr.volcengine.enabled = false;
                    settings.asr.openai_compatible = candidate.clone();
                }
            }
            store.save(&settings)
        })
        .map_err(|_| AsrConfigError::Store)
}

#[cfg(test)]
pub(super) fn save_asr_configuration(
    store: &JsonSettingsStore,
    api_key: &str,
    model: &str,
) -> Result<(), AsrConfigError> {
    save_candidate(
        store,
        &candidate(UiAsrProvider::Volcengine, api_key, "", model)?,
    )
}

#[cfg(test)]
pub(super) fn save_custom_asr_configuration(
    store: &JsonSettingsStore,
    api_key: &str,
    base_url: &str,
    model: &str,
) -> Result<(), AsrConfigError> {
    save_candidate(
        store,
        &candidate(UiAsrProvider::Custom, api_key, base_url, model)?,
    )
}

pub(super) fn volcengine_model_id(model: &str) -> Result<&'static str, AsrConfigError> {
    match model.trim() {
        "" => Err(AsrConfigError::MissingModel),
        VOLCENGINE_ASR_1_MODEL => Ok(VOLCENGINE_ASR_1_MODEL),
        VOLCENGINE_ASR_2_MODEL | VOLCENGINE_LEGACY_MODEL => Ok(VOLCENGINE_ASR_2_MODEL),
        _ => Err(AsrConfigError::UnsupportedModel),
    }
}

pub(super) fn volcengine_api_key_is_valid(api_key: &str) -> bool {
    api_key.len() == 36 && Uuid::parse_str(api_key).is_ok()
}

pub(super) fn clear_asr_configuration(store: &JsonSettingsStore) -> Result<(), SettingsStoreError> {
    store.load_catalog().and_then(|mut catalog| {
        let active = catalog.active.asr.take();
        catalog
            .asr_providers
            .retain(|provider| Some(&provider.id) != active.as_ref());
        store.save_catalog(&catalog)
    })
}

fn select_configured_asr_provider(
    store: &JsonSettingsStore,
    provider: UiAsrProvider,
) -> Result<bool, SettingsStoreError> {
    let mut catalog = store.load_catalog()?;
    let selected = match provider {
        UiAsrProvider::Volcengine => catalog.select_volcengine_asr_provider(),
        UiAsrProvider::Custom => catalog.select_openai_transcriptions_asr_provider(),
    };
    if selected {
        store.save_catalog(&catalog)?;
    }
    Ok(selected)
}

fn apply_provider_status(ui: &AppWindow, store: &JsonSettingsStore, provider: UiAsrProvider) {
    let Ok(settings) = store.load() else {
        apply_status(
            ui,
            false,
            true,
            ui.global::<Translations>()
                .get_common_configuration_load_failed(),
        );
        return;
    };
    let configured = match provider {
        UiAsrProvider::Volcengine => {
            let settings = settings.asr.volcengine;
            !settings.api_key.trim().is_empty()
                && volcengine_api_key_is_valid(settings.api_key.trim())
                && !settings.model.trim().is_empty()
        }
        UiAsrProvider::Custom => {
            let settings = settings.asr.openai_compatible;
            !settings.api_key.trim().is_empty()
                && !settings.base_url.trim().is_empty()
                && !settings.model.trim().is_empty()
        }
    };
    apply_status(
        ui,
        configured,
        false,
        if configured {
            ui.global::<Translations>().get_models_configured()
        } else {
            ui.global::<Translations>().get_models_not_configured()
        },
    );
    apply_pending_test(ui, configured);
    ui.set_asr_draft_error(false);
    ui.set_asr_error_field(UiAsrConfigurationField::None);
}

fn config_error_field(error: AsrConfigError) -> UiAsrConfigurationField {
    match error {
        AsrConfigError::MissingApiKey | AsrConfigError::InvalidApiKey => {
            UiAsrConfigurationField::ApiKey
        }
        AsrConfigError::MissingBaseUrl | AsrConfigError::InvalidBaseUrl => {
            UiAsrConfigurationField::BaseUrl
        }
        AsrConfigError::MissingModel | AsrConfigError::UnsupportedModel => {
            UiAsrConfigurationField::Model
        }
        AsrConfigError::Store => UiAsrConfigurationField::Form,
    }
}

fn asr_test_failure_status(ui: &AppWindow, error: &SpeechRecognitionError) -> SharedString {
    let translations = ui.global::<Translations>();
    match error {
        SpeechRecognitionError::NotConfigured => translations.get_models_enter_api_key(),
        SpeechRecognitionError::Authentication => translations.get_models_test_authentication(),
        SpeechRecognitionError::Quota => translations.get_models_test_quota(),
        SpeechRecognitionError::Transport(_) => translations.get_models_test_transport(),
        SpeechRecognitionError::Protocol(_) => translations.get_models_test_protocol(),
        SpeechRecognitionError::Timeout => translations.get_models_test_timeout(),
    }
}

fn asr_config_error_text(ui: &AppWindow, error: AsrConfigError) -> SharedString {
    let translations = ui.global::<Translations>();
    match error {
        AsrConfigError::MissingApiKey => translations.get_models_enter_api_key(),
        AsrConfigError::InvalidApiKey => translations.get_models_invalid_api_key(),
        AsrConfigError::MissingBaseUrl => translations.get_models_enter_service_url(),
        AsrConfigError::InvalidBaseUrl => translations.get_models_invalid_service_url(),
        AsrConfigError::MissingModel => translations.get_models_enter_model_name(),
        AsrConfigError::UnsupportedModel => translations.get_models_unsupported_volcengine_model(),
        AsrConfigError::Store => translations.get_common_save_failed(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    const CURRENT_KEY: &str = "123e4567-e89b-42d3-a456-426614174000";
    const CANDIDATE_KEY: &str = "223e4567-e89b-42d3-a456-426614174000";

    #[test]
    fn authentication_network_and_quota_failures_keep_the_current_configuration() {
        let directory = std::env::temp_dir().join(format!("saymore-asr-atomic-{}", Uuid::new_v4()));
        let store = JsonSettingsStore::at_path(directory.join("providers.json"));
        assert_eq!(
            Ok(()),
            save_asr_configuration(&store, CURRENT_KEY, VOLCENGINE_ASR_2_MODEL)
        );
        let candidate = candidate(
            UiAsrProvider::Volcengine,
            CANDIDATE_KEY,
            "",
            VOLCENGINE_ASR_2_MODEL,
        );
        let Ok(candidate) = candidate else {
            panic!("candidate should be valid");
        };

        for error in [
            SpeechRecognitionError::Authentication,
            SpeechRecognitionError::Transport("offline".to_owned()),
            SpeechRecognitionError::Quota,
        ] {
            assert!(test_and_save(&store, &candidate, |_| Err(error)).is_err());
            assert_eq!(
                Some(CURRENT_KEY.to_owned()),
                store
                    .load()
                    .ok()
                    .map(|settings| settings.asr.volcengine.api_key)
            );
        }
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn configured_providers_can_be_switched_without_losing_either_configuration() {
        let directory = std::env::temp_dir().join(format!("saymore-asr-switch-{}", Uuid::new_v4()));
        let store = JsonSettingsStore::at_path(directory.join("providers.json"));
        assert_eq!(
            Ok(()),
            save_asr_configuration(&store, CURRENT_KEY, VOLCENGINE_ASR_2_MODEL)
        );
        assert_eq!(
            Ok(()),
            save_custom_asr_configuration(
                &store,
                "custom-key",
                "https://example.com/v1",
                "whisper-1",
            )
        );

        assert_eq!(
            Ok(true),
            select_configured_asr_provider(&store, UiAsrProvider::Volcengine)
        );
        let volcengine_settings = store.load();
        assert!(volcengine_settings.is_ok());
        let volcengine_settings = volcengine_settings.unwrap_or_default();
        assert!(volcengine_settings.asr.volcengine.enabled);
        assert!(!volcengine_settings.asr.openai_compatible.enabled);

        assert_eq!(
            Ok(true),
            select_configured_asr_provider(&store, UiAsrProvider::Custom)
        );
        let custom_settings = store.load();
        assert!(custom_settings.is_ok());
        let custom_settings = custom_settings.unwrap_or_default();
        assert!(!custom_settings.asr.volcengine.enabled);
        assert!(custom_settings.asr.openai_compatible.enabled);
        assert_eq!(CURRENT_KEY, custom_settings.asr.volcengine.api_key);
        assert_eq!("custom-key", custom_settings.asr.openai_compatible.api_key);

        let _ = fs::remove_dir_all(directory);
    }
}
