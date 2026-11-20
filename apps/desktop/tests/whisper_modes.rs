use slint::{
    ComponentHandle, Timer,
    platform::{Key, WindowEvent},
};
use std::{sync::Arc, time::Duration};
use template_app::{ProviderCatalog, ProviderConfigStore, WhisperRecognitionMode};
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
#[path = "../src/settings_ui/whisper_mode.rs"]
mod binding;
#[path = "support/model_controls.rs"]
mod controls;
#[path = "support/whisper_metrics.rs"]
mod metrics;
#[path = "support/whisper_controls.rs"]
mod remove_button;
#[path = "support/mode_ui.rs"]
mod support;
use controls::{
    AccessibleRole::{Button, RadioButton, Text},
    bounds, click,
};
use support::require;

// Stand in for the local runtime so the UI flow never downloads or runs a real model.
fn wire_model_actions(window: &ui::AppWindow) {
    let weak = window.as_weak();
    window.on_request_local_model_download(move |kind| {
        require(kind == ui::AsrProviderCardKind::WhisperLargeV3Turbo);
        if let Some(window) = weak.upgrade() {
            window.set_whisper_download_state(ui::LocalModelDownloadState::Downloaded);
            window.set_whisper_installed_size("832 MB".into());
        }
    });
    let weak = window.as_weak();
    window.on_request_local_model_delete(move |kind| {
        require(kind == ui::AsrProviderCardKind::WhisperLargeV3Turbo);
        if let Some(window) = weak.upgrade() {
            window.set_whisper_download_state(ui::LocalModelDownloadState::NotDownloaded);
        }
    });
    let weak = window.as_weak();
    window.on_request_local_model_test(move |kind| {
        require(kind == ui::AsrProviderCardKind::WhisperLargeV3Turbo);
        if let Some(window) = weak.upgrade() {
            window.set_whisper_test(ui::LocalModelTestState {
                status: ui::LocalModelTestStatus::Succeeded,
                transcript: "可以通过语音进行人机交互".into(),
                ..Default::default()
            });
        }
    });
    let weak = window.as_weak();
    window.on_request_local_model_selection(move |kind| {
        require(kind == ui::AsrProviderCardKind::WhisperLargeV3Turbo);
        if let Some(window) = weak.upgrade() {
            window.set_whisper_selected(true);
        }
    });
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/whisper-streaming-ui");
    std::fs::create_dir_all(&root)?;
    let path = root.join(format!("{}.json", uuid::Uuid::new_v4()));
    let store = Arc::new(JsonSettingsStore::at_path(path.clone()));
    let mut catalog = ProviderCatalog::default();
    catalog.select_qwen3_asr_provider();
    store.save_catalog(&catalog)?;
    let window = ui::AppWindow::new()?;
    slint::select_bundled_translation("en")?;
    binding::wire(&window, store.clone());
    wire_model_actions(&window);
    window.set_current_page(ui::AppPage::Models);
    window.set_model_tab(0);
    window.set_asr_provider_cards(slint::ModelRc::new(slint::VecModel::from(vec![
        ui::AsrProviderCardSpec {
            kind: ui::AsrProviderCardKind::WhisperLargeV3Turbo,
            available: true,
        },
    ])));
    window.set_whisper_download_size("834 MB".into());
    window.set_whisper_download_supported(true);
    window.set_whisper_adapter_ready(true);
    window
        .window()
        .set_size(slint::LogicalSize::new(820.0, 560.0));
    window.show()?;
    remove_button::use_scripted_pointer(window.window());
    window.window().try_dispatch_event(WindowEvent::Resized {
        size: slint::LogicalSize::new(820.0, 560.0),
    })?;
    let overlay = ui::RecordingOverlay::new()?;
    overlay.set_preview_enabled(true);
    overlay.set_revealed(true);
    // Match the production overlay configuration before showing its native window.
    overlay
        .window()
        .set_size(slint::LogicalSize::new(360.0, 35.0));
    overlay.set_preview_text("这是流式识别预览。The text updates while you speak.".into());
    let preview = overlay.as_weak();
    let weak = window.as_weak();
    let observed = store.clone();
    Timer::single_shot(Duration::from_millis(150), move || {
        let Some(window) = weak.upgrade() else {
            std::process::exit(1);
        };
        click(&window, Button, "Whisper large-v3-turbo");
        Timer::single_shot(Duration::from_millis(100), move || {
            install(&window);
            check_test_result_and_mode(&window, &observed);
            remove_button::check_remove_gesture(&window);

            window.set_whisper_test(Default::default());
            let size = slint::LogicalSize::new(920.0, 700.0);
            window.window().set_size(size);
            require(
                window
                    .window()
                    .try_dispatch_event(WindowEvent::Resized { size })
                    .is_ok(),
            );
            Timer::single_shot(Duration::from_millis(100), move || {
                install(&window);
                check_test_result_and_mode(&window, &observed);
                remove_button::check_remove_tooltip(window, move |window| finish(window, preview));
            });
        });
    });
    slint::run_event_loop()?;
    let restored = ui::AppWindow::new()?;
    binding::wire(&restored, store);
    require(restored.get_whisper_recognition_mode() == ui::LocalRecognitionMode::NonStreaming);
    std::fs::remove_file(path)?;
    Ok(())
}

fn install(window: &ui::AppWindow) {
    bounds(window, Text, "Estimated memory");
    require(controls::visible_bounds(window, Button, "Remove model").is_none());
    click(window, Button, "Download · 834 MB");
    require(window.get_whisper_download_state() == ui::LocalModelDownloadState::Downloaded);
    metrics::check_installed(window);
}

fn panel_span(window: &ui::AppWindow) -> f32 {
    bounds(window, Button, "Use model").max_y() - bounds(window, Button, "Close").min_y()
}

fn check_test_result_and_mode(window: &ui::AppWindow, observed: &JsonSettingsStore) {
    let compact = panel_span(window);
    click(window, Button, "Run");
    require(
        window.get_whisper_test().status == ui::LocalModelTestStatus::Succeeded
            && !window.get_whisper_selected(),
    );
    require(panel_span(window) > compact);

    let subtitle = bounds(window, Text, "OpenAI · Offline multilingual recognition");
    let mode = bounds(window, Text, "Recognition mode");
    let files = bounds(window, Text, "Files");
    let test = bounds(window, Text, "Test");
    let transcript = bounds(window, Text, "Transcript: 可以通过语音进行人机交互");
    let delete = bounds(window, Button, "Remove model");
    require(subtitle.max_y() <= mode.min_y());
    require(mode.max_y() <= files.min_y());
    require(files.max_y() <= test.min_y());
    require(test.max_y() <= transcript.min_y());
    require(transcript.max_y() <= delete.min_y());

    click(window, RadioButton, "Streaming");
    require(window.get_whisper_recognition_mode() == ui::LocalRecognitionMode::Streaming);
    press_key(window, Key::LeftArrow);
    require(window.get_whisper_recognition_mode() == ui::LocalRecognitionMode::NonStreaming);
    press_key(window, Key::RightArrow);
    require(window.get_whisper_recognition_mode() == ui::LocalRecognitionMode::Streaming);
    require(observed.load_catalog().is_ok_and(|catalog| {
        catalog.qwen3_asr_is_active()
            && catalog.whisper_recognition_mode() == Ok(WhisperRecognitionMode::Streaming)
    }));
}

fn finish(window: ui::AppWindow, preview: slint::Weak<ui::RecordingOverlay>) {
    let weak = window.as_weak();
    support::after_snapshot(
        &window,
        "whisper-streaming-ui",
        "mode-selector",
        move || {
            if let Some(window) = weak.upgrade() {
                click(&window, RadioButton, "Non-streaming");
                require(
                    window.get_whisper_recognition_mode() == ui::LocalRecognitionMode::NonStreaming,
                );
                click(&window, Button, "Use model");
                require(window.get_whisper_selected());
                metrics::check_delete_unavailable(&window);
                metrics::check_active_memory(&window);
            }
            if let Some(overlay) = preview.upgrade() {
                support::show_preview_and_finish(&overlay, "whisper-streaming-ui");
            }
        },
    );
}

fn press_key(window: &ui::AppWindow, key: Key) {
    let text = slint::SharedString::from(key);
    for event in [
        WindowEvent::KeyPressed { text: text.clone() },
        WindowEvent::KeyReleased { text },
    ] {
        require(window.window().try_dispatch_event(event).is_ok());
    }
}
