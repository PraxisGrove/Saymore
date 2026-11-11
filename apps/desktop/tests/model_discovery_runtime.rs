use std::{cell::Cell, io::Read as _, rc::Rc, sync::Arc, time::Duration};

use httpmock::{Method::GET, MockServer};
use slint::{ComponentHandle, Model, Timer, TimerMode};
use template_app::{CloudAsrPreset, LlmProviderPreset, ProviderConfigStore, SettingsStore};
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

fn check_cloud_asr_presets(ui: &ui::AppWindow, store: &JsonSettingsStore) {
    let saved = require(store.load_catalog());
    ui.set_configured_custom_asr_kind(ui::AsrProviderCardKind::Openai);
    ui.set_custom_asr_base_url(CloudAsrPreset::OpenAi.base_url().into());
    ui.set_custom_asr_api_key("old-provider-key".into());
    ui.set_custom_asr_model("old-model".into());
    ui.invoke_prepare_cloud_asr(ui::CloudAsrService::Groq);
    assert_eq!("Groq", ui.get_custom_asr_service_name().as_str());
    assert_eq!(
        ui::AsrProviderCardKind::Groq,
        ui.get_draft_custom_asr_kind()
    );
    assert_eq!(
        ui::AsrProviderCardKind::Openai,
        ui.get_configured_custom_asr_kind()
    );
    assert_eq!(
        CloudAsrPreset::Groq.base_url(),
        ui.get_custom_asr_base_url().as_str()
    );
    assert_eq!(
        CloudAsrPreset::Groq.default_model(),
        ui.get_custom_asr_model().as_str()
    );
    assert!(ui.get_custom_asr_api_key().is_empty());
    ui.set_custom_asr_api_key("groq-key".into());
    ui.set_custom_asr_model("whisper-large-v3".into());
    ui.invoke_prepare_cloud_asr(ui::CloudAsrService::Groq);
    assert_eq!("groq-key", ui.get_custom_asr_api_key().as_str());
    assert_eq!("whisper-large-v3", ui.get_custom_asr_model().as_str());
    ui.set_asr_testing(true);
    ui.invoke_prepare_cloud_asr(ui::CloudAsrService::Openai);
    assert_eq!(
        CloudAsrPreset::Groq.base_url(),
        ui.get_custom_asr_base_url().as_str()
    );
    ui.set_asr_testing(false);
    assert_eq!(saved, require(store.load_catalog()));

    // Known services offer speech models without fetching unrelated chat models.
    ui.set_model_tab(0);
    ui.invoke_refresh_models();
    assert!(models(ui).contains(&"whisper-large-v3-turbo".to_owned()));
    assert_eq!("whisper-large-v3", ui.get_custom_asr_model().as_str());
    assert!(!ui.get_model_discovery_loading());
}

fn main() {
    let ui = require(ui::AppWindow::new());
    require(ui.show());
    let directory =
        std::env::temp_dir().join(format!("saymore-discovery-{}", uuid::Uuid::new_v4()));
    let store = Arc::new(JsonSettingsStore::at_path(directory.join("providers.json")));
    require(store.ensure_exists());
    settings_ui::wire(&ui, Arc::clone(&store));
    check_magpie_connection_handoff(&ui, &store);
    check_cloud_asr_presets(&ui, &store);
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

    // A keyless local Custom model choice saves without enabling polishing;
    // unsaved application credentials still keep the model in the draft.
    ui.invoke_select_llm_model(ui::LlmProvider::Custom, "local-polisher".into());
    assert!(!ui.get_llm_draft_error());
    assert_eq!(
        "custom-model",
        require(
            require(store.load_catalog())
                .llm_provider_settings(LlmProviderPreset::Custom,)
                .ok_or("missing custom provider")
        )
        .model
    );
    ui.set_llm_draft_api_key("unsaved-key".into());
    ui.invoke_select_llm_model(ui::LlmProvider::Custom, "local-polisher".into());
    assert!(!ui.get_llm_draft_error());
    assert!(!require(store.load()).llm.enabled);
    assert_eq!(
        "custom-model",
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

fn check_magpie_connection_handoff(ui: &ui::AppWindow, store: &JsonSettingsStore) {
    let mut catalog = require(store.load_catalog());
    catalog.save_custom_llm_provider_config(
        LlmProviderPreset::Magpie.base_url(),
        "custom-key",
        "custom-model",
    );
    require(store.save_catalog(&catalog));
    let path = std::env::temp_dir().join(format!("saymore-magpie-{}", uuid::Uuid::new_v4()));
    let magpie_store = JsonSettingsStore::at_path(path.join("config.json"));
    require(magpie_store.save_catalog(&catalog));
    let serialized = require(std::fs::read(path.join("config.json")));
    let mut config: serde_json::Value = require(serde_json::from_slice(&serialized));
    require(
        config
            .as_object_mut()
            .ok_or("test settings are not an object")
            .map(|config| {
                config.insert(
                    "integrations".to_owned(),
                    serde_json::json!({
                        "magpie": {
                            "base_url": "http://127.0.0.1:3425/v1",
                            "model": "provider/polisher",
                            "profile": "portable"
                        },
                        "another-application": {"keep": true}
                    }),
                )
            }),
    );
    let serialized = require(serde_json::to_vec_pretty(&config));
    require(std::fs::write(path.join("config.json"), serialized));
    let catalog = require(magpie_store.load_catalog());
    assert_eq!(None, catalog.active.llm);
    assert_eq!(
        Some("provider/polisher"),
        catalog.configured_llm_provider_model(LlmProviderPreset::Magpie)
    );
    assert!(!require(magpie_store.load()).llm.enabled);

    // Keep the production URL fixed while routing only this test process away
    // from a real Magpie gateway that may already listen on port 3425.
    let listener = require(
        std::net::TcpListener::bind("127.0.0.1:0")
            .map_err(|error| format!("Magpie mock proxy cannot bind a loopback port: {error}")),
    );
    let proxy_address = require(
        listener
            .local_addr()
            .map_err(|error| format!("Magpie mock proxy has no loopback address: {error}")),
    );
    // SAFETY: this single-threaded test sets its proxy before spawning the
    // only request worker. All later mock requests are local loopback targets.
    unsafe {
        std::env::set_var("HTTP_PROXY", format!("http://{proxy_address}"));
        std::env::remove_var("http_proxy");
        std::env::remove_var("ALL_PROXY");
        std::env::remove_var("all_proxy");
        std::env::set_var("NO_PROXY", "");
        std::env::remove_var("no_proxy");
    }
    let response = std::thread::spawn(move || -> Result<(), String> {
        let (mut stream, _) = listener.accept().map_err(|error| error.to_string())?;
        let mut reader = std::io::BufReader::new(match stream.try_clone() {
            Ok(stream) => stream,
            Err(error) => return Err(error.to_string()),
        });
        use std::io::{BufRead, Write};
        let mut lines = Vec::new();
        for line in reader.by_ref().lines() {
            match line {
                Ok(line) if line.is_empty() => break,
                Ok(line) => lines.push(line),
                Err(error) => return Err(error.to_string()),
            }
        }
        assert_eq!(
            Some("GET http://127.0.0.1:3425/v1/models HTTP/1.1"),
            lines.first().map(String::as_str)
        );
        assert!(
            lines
                .iter()
                .any(|line| line.eq_ignore_ascii_case("host: 127.0.0.1:3425"))
        );
        assert!(
            !lines
                .iter()
                .any(|line| line.to_ascii_lowercase().starts_with("authorization:"))
        );
        assert!(!lines.iter().any(|line| line.contains("untrusted.example")));
        let body = r#"{"data":[{"id":"provider/polisher"},{"id":"other/model"}]}"#;
        let reply = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream
            .write_all(reply.as_bytes())
            .map_err(|error| error.to_string())
    });
    ui.set_model_tab(1);
    ui.set_llm_modal_visible(true);
    ui.set_llm_provider(ui::LlmProvider::Magpie);
    ui.set_llm_draft_base_url("https://untrusted.example/v1".into());
    ui.set_llm_draft_api_key("".into());
    ui.set_llm_draft_model("provider/polisher".into());
    fetch(ui);
    // SAFETY: the sole request worker has returned and no other test thread is
    // constructing a client while this process-wide proxy is being reset.
    unsafe {
        std::env::remove_var("HTTP_PROXY");
        std::env::remove_var("NO_PROXY");
    }
    match response.join() {
        Ok(result) => require(result),
        Err(_) => require(Err::<(), _>("Magpie mock thread failed")),
    }
    assert_eq!(vec!["other/model", "provider/polisher"], models(ui));
    assert_eq!("provider/polisher", ui.get_llm_draft_model().as_str());
    assert!(!ui.get_model_discovery_error());
    let saved = require(store.load_catalog());
    assert_eq!(
        Some("custom-model"),
        saved.configured_llm_provider_model(LlmProviderPreset::Custom)
    );
    assert_eq!(None, saved.active.llm);
    assert!(!require(store.load()).llm.enabled);
    // Discovery may seed a separate draft catalog but never configures, selects,
    // enables, or replaces Custom with Magpie.
    assert_eq!(
        None,
        saved.configured_llm_provider_model(LlmProviderPreset::Magpie)
    );
    require(std::fs::remove_dir_all(path));
    ui.set_llm_draft_model("".into());
    ui.set_llm_draft_api_key("".into());
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
