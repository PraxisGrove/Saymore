use std::{cell::Cell, rc::Rc, sync::Arc, time::Duration};

use httpmock::{Method::GET, MockServer};
use slint::{ComponentHandle, Model, Timer, TimerMode};
use template_app::{LlmProviderPreset, ProviderConfigStore};
use template_infra::JsonSettingsStore;

#[allow(
    clippy::panic,
    clippy::todo,
    clippy::unimplemented,
    clippy::unwrap_used
)]
mod ui {
    slint::include_modules!();
}

#[path = "support/model_discovery_runtime.rs"]
mod settings_ui;

fn main() {
    let ui = require(ui::AppWindow::new());
    require(ui.show());
    let directory =
        std::env::temp_dir().join(format!("saymore-discovery-{}", uuid::Uuid::new_v4()));
    let store = Arc::new(JsonSettingsStore::at_path(directory.join("providers.json")));
    require(store.ensure_exists());
    settings_ui::wire(&ui, Arc::clone(&store));
    let server = MockServer::start();
    let mut response = server.mock(|when, then| {
        when.method(GET)
            .path("/v1/models")
            .header("authorization", "Bearer speech-key");
        then.status(200).json_body(serde_json::json!({
            "data": [{"id": "model-b"}, {"id": " model-a "}, {"id": "model-b"}]
        }));
    });

    ui.set_asr_provider(ui::AsrProvider::Custom);
    ui.set_custom_asr_base_url(server.url("/v1/audio/transcriptions").into());
    ui.set_custom_asr_api_key("speech-key".into());
    ui.set_custom_asr_model("model-b".into());
    ui.set_focus_asr_config(true);
    ui.set_current_page(ui::AppPage::Models);
    fetch(&ui);
    response.assert();
    assert_eq!(vec!["model-a", "model-b"], models(&ui));
    assert_eq!("model-b", ui.get_custom_asr_model().as_str());
    assert!(!ui.get_model_discovery_error());

    // A fetched directory remains available when a user types their own model.
    ui.set_custom_asr_model("manual-speech-model".into());
    assert_eq!(vec!["model-a", "model-b"], models(&ui));
    response.delete();
    let mut rejected = server.mock(|when, then| {
        when.method(GET).path("/v1/models");
        then.status(401);
    });
    fetch(&ui);
    rejected.assert();
    assert!(ui.get_model_discovery_error());
    assert_eq!(vec!["model-a", "model-b"], models(&ui));
    assert_eq!("manual-speech-model", ui.get_custom_asr_model().as_str());
    rejected.delete();

    let mut local = server.mock(|when, then| {
        when.method(GET)
            .path("/v1/models")
            .header_missing("authorization");
        then.status(200)
            .json_body(serde_json::json!({"data": [{"id": "local-polisher"}]}));
    });
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed {
            text: slint::platform::Key::Escape.into(),
        });
    ui.set_llm_modal_visible(true);
    ui.set_model_tab(1);
    ui.set_llm_provider(ui::LlmProvider::Custom);
    ui.set_llm_draft_base_url(server.url("/v1/chat/completions").into());
    ui.set_llm_draft_api_key("".into());
    fetch(&ui);
    local.assert();
    assert_eq!(vec!["local-polisher"], models(&ui));
    assert_eq!("local-polisher", ui.get_llm_draft_model().as_str());
    assert!(!ui.get_model_discovery_error());
    let catalog = require(store.load_catalog());
    assert_eq!(
        vec!["local-polisher"],
        require(
            catalog
                .llm_model_catalog(
                    LlmProviderPreset::Custom,
                    &server.url("/v1/models"),
                    template_app::ChatCompletionsProfile::Portable,
                )
                .ok_or("missing fetched catalog")
        )
        .models
    );

    // Choosing a fetched model with unsaved credentials stays a draft.
    ui.invoke_select_llm_model(ui::LlmProvider::Custom, "local-polisher".into());
    assert!(!ui.get_llm_draft_error());
    assert_eq!(
        "",
        require(
            require(store.load_catalog())
                .llm_provider_settings(LlmProviderPreset::Custom,)
                .ok_or("missing custom provider")
        )
        .model
    );

    let mut configured = require(store.load_catalog());
    configured.save_custom_llm_provider_config(
        &server.url("/v1/chat/completions"),
        "saved-key",
        "saved-model",
    );
    require(store.save_catalog(&configured));
    ui.set_llm_saved_api_key("saved-key".into());
    ui.set_llm_saved_base_url(server.url("/v1/chat/completions").into());
    ui.set_llm_saved_model("saved-model".into());
    ui.set_llm_draft_api_key("temporary-key".into());
    local.delete();
    let mut draft_directory = server.mock(|when, then| {
        when.method(GET)
            .path("/v1/models")
            .header("authorization", "Bearer temporary-key");
        then.status(200)
            .json_body(serde_json::json!({"data": [{"id": "temporary-model"}]}));
    });
    fetch(&ui);
    draft_directory.assert();
    assert_eq!("temporary-model", ui.get_llm_draft_model().as_str());
    // Cancel/reopen restores the confirmed model even if the cached directory came from a draft key.
    ui.set_llm_draft_api_key(ui.get_llm_saved_api_key());
    settings_ui::restore_catalog(&ui, &store);
    assert_eq!("saved-model", ui.get_llm_draft_model().as_str());
    assert_eq!("saved-key", ui.get_llm_draft_api_key().as_str());
    assert!(models(&ui).iter().any(|model| model == "saved-model"));
    draft_directory.delete();
    ui.set_llm_draft_api_key("".into());
    ui.set_llm_draft_model("local-polisher".into());
    ui.set_available_models(slint::ModelRc::new(slint::VecModel::from(vec![
        "local-polisher".into(),
    ])));

    let replacement = server.mock(|when, then| {
        when.method(GET).path("/v1/models");
        then.status(200).json_body(serde_json::json!({
            "data": [{"id": "replacement-polisher"}]
        }));
    });
    let config_path = directory.join("providers.json");
    let backup_path = directory.join("providers-backup.json");
    require(std::fs::rename(&config_path, &backup_path));
    require(std::fs::create_dir(&config_path));
    fetch(&ui);
    replacement.assert();
    assert!(ui.get_model_discovery_error());
    assert_eq!(vec!["local-polisher"], models(&ui));
    assert_eq!("local-polisher", ui.get_llm_draft_model().as_str());
    require(std::fs::remove_dir(&config_path));
    require(std::fs::rename(&backup_path, &config_path));

    ui.set_llm_provider(ui::LlmProvider::Qwen);
    ui.set_llm_draft_model("qwen-plus".into());
    fetch(&ui);
    ui.invoke_select_llm_model(ui::LlmProvider::Qwen, "qwen-flash".into());
    assert!(!ui.get_llm_draft_error());
    assert_eq!(vec!["qwen-plus", "qwen-flash"], models(&ui));

    // Switching tabs releases loading even while a response is outstanding.
    ui.set_llm_provider(ui::LlmProvider::Custom);
    settle();
    ui.invoke_refresh_models();
    assert!(ui.get_model_discovery_loading());
    ui.set_model_tab(0);
    settle();
    assert!(!ui.get_model_discovery_loading());

    // Closing a modal invalidates the request and immediately releases loading.
    ui.set_model_tab(1);
    ui.set_llm_modal_visible(true);
    settle();
    ui.invoke_refresh_models();
    assert!(ui.get_model_discovery_loading());
    ui.set_llm_modal_visible(false);
    settle();
    assert!(!ui.get_model_discovery_loading());
    require(std::fs::remove_dir_all(directory));
}

fn models(ui: &ui::AppWindow) -> Vec<String> {
    ui.get_available_models()
        .iter()
        .map(|model| model.to_string())
        .collect()
}

fn fetch(ui: &ui::AppWindow) {
    settle();
    ui.invoke_refresh_models();
    let poll = Timer::default();
    let weak = ui.as_weak();
    poll.start(TimerMode::Repeated, Duration::from_millis(10), move || {
        if let Some(ui) = weak.upgrade()
            && !ui.get_model_discovery_loading()
        {
            require(slint::quit_event_loop());
        }
    });
    let timed_out = Rc::new(Cell::new(false));
    let timeout = Timer::default();
    let timeout_state = Rc::clone(&timed_out);
    timeout.start(TimerMode::SingleShot, Duration::from_secs(15), move || {
        timeout_state.set(true);
        require(slint::quit_event_loop());
    });
    require(slint::run_event_loop());
    assert!(!timed_out.get(), "model discovery did not complete");
}

fn settle() {
    Timer::single_shot(Duration::ZERO, || {
        Timer::single_shot(Duration::ZERO, || require(slint::quit_event_loop()));
    });
    require(slint::run_event_loop());
}

fn require<T, E: std::fmt::Display>(result: Result<T, E>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => {
            eprintln!("model discovery runtime test failed: {error}");
            std::process::exit(1);
        }
    }
}
