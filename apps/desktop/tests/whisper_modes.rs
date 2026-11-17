use slint::{ComponentHandle, Timer, platform::WindowEvent};
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
#[path = "support/mode_ui.rs"]
mod support;
use support::{click, require, save_snapshot};

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
    binding::wire(&window, store.clone());
    window.set_current_page(ui::AppPage::Models);
    window.set_model_tab(0);
    window.set_asr_provider_cards(slint::ModelRc::new(slint::VecModel::from(vec![
        ui::AsrProviderCardSpec {
            kind: ui::AsrProviderCardKind::WhisperLargeV3Turbo,
            available: true,
        },
    ])));
    window.set_whisper_download_supported(true);
    window
        .window()
        .set_size(slint::LogicalSize::new(920.0, 600.0));
    window.show()?;
    window.window().try_dispatch_event(WindowEvent::Resized {
        size: slint::LogicalSize::new(920.0, 600.0),
    })?;
    let overlay = ui::RecordingOverlay::new()?;
    overlay.set_preview_enabled(true);
    overlay.set_revealed(true);
    // Match the production overlay configuration before showing its native window.
    overlay
        .window()
        .set_size(slint::LogicalSize::new(360.0, 128.0));
    overlay.set_preview_text("这是流式识别预览。The text updates while you speak.".into());
    let preview = overlay.as_weak();
    let weak = window.as_weak();
    let observed = store.clone();
    Timer::single_shot(Duration::from_millis(150), move || {
        let Some(window) = weak.upgrade() else {
            std::process::exit(1);
        };
        click(&window, 350.0, 200.0);
        Timer::single_shot(Duration::from_millis(100), move || {
            support::click_mode(&window, ui::LocalRecognitionMode::Streaming);
            require(window.get_whisper_recognition_mode() == ui::LocalRecognitionMode::Streaming);
            require(observed.load_catalog().is_ok_and(|catalog| {
                catalog.qwen3_asr_is_active()
                    && catalog.whisper_recognition_mode() == Ok(WhisperRecognitionMode::Streaming)
            }));
            let weak = window.as_weak();
            support::after_frame(window.window(), move |frame| {
                if let Some(window) = weak.upgrade() {
                    if matches!(frame, support::FrameStatus::Rendered) {
                        save_snapshot(window.window(), "whisper-streaming-ui", "mode-selector");
                    }
                    support::click_mode(&window, ui::LocalRecognitionMode::NonStreaming);
                    require(
                        window.get_whisper_recognition_mode()
                            == ui::LocalRecognitionMode::NonStreaming,
                    );
                }
                if let Some(overlay) = preview.upgrade() {
                    support::show_preview_and_finish(&overlay, "whisper-streaming-ui");
                }
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
