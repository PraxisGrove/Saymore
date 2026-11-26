use slint::{ComponentHandle, Timer, platform::WindowEvent};
use std::{sync::Arc, time::Duration};
use template_app::{ProviderCatalog, ProviderConfigStore, QwenRecognitionMode};
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
#[path = "../src/settings_ui/qwen_mode.rs"]
mod binding;
#[path = "support/model_controls.rs"]
mod controls;
#[path = "support/mode_ui.rs"]
mod support;
use controls::{
    AccessibleRole::{Button, RadioButton, Text},
    bounds, click, visible_bounds,
};
use support::require;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/qwen-streaming-ui");
    std::fs::create_dir_all(&root)?;
    let path = root.join(format!("{}.json", uuid::Uuid::new_v4()));
    let store = Arc::new(JsonSettingsStore::at_path(path.clone()));
    let mut catalog = ProviderCatalog::default();
    catalog.select_whisper_provider();
    store.save_catalog(&catalog)?;
    let window = ui::AppWindow::new()?;
    slint::select_bundled_translation("en")?;
    binding::wire(&window, store.clone());
    window.set_current_page(ui::AppPage::Models);
    window.set_model_tab(0);
    window.set_asr_provider_cards(slint::ModelRc::new(slint::VecModel::from(vec![
        ui::AsrProviderCardSpec {
            kind: ui::AsrProviderCardKind::Qwen3Asr,
            available: true,
        },
    ])));
    window.set_qwen3_asr_download_supported(true);
    window.set_qwen3_asr_adapter_ready(true);
    window.set_qwen3_asr_download_size("2.03 GB".into());
    let tapped = window.as_weak();
    window.on_request_local_model_download(move |kind| {
        require(kind == ui::AsrProviderCardKind::Qwen3Asr);
        if let Some(window) = tapped.upgrade() {
            window.set_qwen3_asr_download_state(ui::LocalModelDownloadState::Downloaded);
            window.set_qwen3_asr_installed_size("2.03 GB".into());
        }
    });
    window
        .window()
        .set_size(slint::LogicalSize::new(920.0, 600.0));
    window.show()?;
    window.window().try_dispatch_event(WindowEvent::Resized {
        size: slint::LogicalSize::new(920.0, 600.0),
    })?;
    let overlay = ui::RecordingOverlay::new()?;
    let preview = overlay.as_weak();
    let weak = window.as_weak();
    let observed = store.clone();
    Timer::single_shot(Duration::from_millis(150), move || {
        let Some(window) = weak.upgrade() else {
            std::process::exit(1);
        };
        click(&window, Button, "Qwen3-ASR 1.7B");
        require(visible_bounds(&window, RadioButton, "Streaming").is_none());
        click(&window, Button, "Download · 2.03 GB");
        bounds(&window, Text, "Recognition process memory");
        click(&window, RadioButton, "Streaming");
        require(window.get_qwen_recognition_mode() == ui::LocalRecognitionMode::Streaming);
        require(observed.load_catalog().is_ok_and(|catalog| {
            catalog.whisper_is_active()
                && catalog.qwen_recognition_mode() == Ok(QwenRecognitionMode::Streaming)
        }));
        let weak = window.as_weak();
        support::after_snapshot(&window, "qwen-streaming-ui", "mode-selector", move || {
            if let Some(window) = weak.upgrade() {
                click(&window, RadioButton, "Non-streaming");
                require(
                    window.get_qwen_recognition_mode() == ui::LocalRecognitionMode::NonStreaming,
                );
                if let Some(overlay) = preview.upgrade() {
                    support::show_preview_and_finish(&window, &overlay, "qwen-streaming-ui");
                }
            }
        });
    });
    slint::run_event_loop()?;
    let restored = ui::AppWindow::new()?;
    binding::wire(&restored, store);
    require(restored.get_qwen_recognition_mode() == ui::LocalRecognitionMode::NonStreaming);
    std::fs::remove_file(path)?;
    Ok(())
}
