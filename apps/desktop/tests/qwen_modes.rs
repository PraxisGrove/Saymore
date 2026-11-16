use slint::{
    ComponentHandle, Timer,
    platform::{PointerEventButton, WindowEvent},
};
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
    window.show()?;
    window.window().try_dispatch_event(WindowEvent::Resized {
        size: slint::LogicalSize::new(920.0, 700.0),
    })?;
    let overlay = ui::RecordingOverlay::new()?;
    overlay.set_preview_enabled(true);
    overlay.set_preview_text("这是流式识别预览。The text updates while you speak.".into());
    let preview = overlay.as_weak();
    let weak = window.as_weak();
    let observed = store.clone();
    Timer::single_shot(Duration::from_millis(150), move || {
        let Some(window) = weak.upgrade() else {
            std::process::exit(1);
        };
        click(&window, 350.0, 200.0);
        click(&window, 690.0, 256.0);
        require(window.get_qwen_recognition_mode() == ui::QwenRecognitionMode::Streaming);
        require(observed.load_catalog().is_ok_and(|catalog| {
            catalog.whisper_is_active()
                && catalog.qwen_recognition_mode() == Ok(QwenRecognitionMode::Streaming)
        }));
        let weak = window.as_weak();
        Timer::single_shot(Duration::from_millis(100), move || {
            if let Some(window) = weak.upgrade() {
                save_snapshot(window.window(), "mode-selector");
                click(&window, 590.0, 256.0);
                require(
                    window.get_qwen_recognition_mode() == ui::QwenRecognitionMode::NonStreaming,
                );
            }
            if let Some(overlay) = preview.upgrade() {
                require(overlay.show().is_ok());
            }
            Timer::single_shot(Duration::from_millis(60), move || {
                if let Some(overlay) = preview.upgrade() {
                    overlay.set_revealed(true);
                }
                Timer::single_shot(Duration::from_millis(200), move || {
                    if let Some(overlay) = preview.upgrade() {
                        save_snapshot(overlay.window(), "streaming-preview");
                    }
                    let _ = slint::quit_event_loop();
                });
            });
        });
    });
    slint::run_event_loop()?;
    let restored = ui::AppWindow::new()?;
    binding::wire(&restored, store);
    require(restored.get_qwen_recognition_mode() == ui::QwenRecognitionMode::NonStreaming);
    std::fs::remove_file(path)?;
    Ok(())
}

fn require(valid: bool) {
    if !valid {
        eprintln!("Qwen mode UI contract failed");
        std::process::exit(1);
    }
}
fn click(ui: &ui::AppWindow, x: f32, y: f32) {
    for event in [
        WindowEvent::PointerPressed {
            position: slint::LogicalPosition::new(x, y),
            button: PointerEventButton::Left,
        },
        WindowEvent::PointerReleased {
            position: slint::LogicalPosition::new(x, y),
            button: PointerEventButton::Left,
        },
    ] {
        require(ui.window().try_dispatch_event(event).is_ok());
    }
}
fn save_snapshot(window: &slint::Window, name: &str) {
    // Snapshot artifacts are optional on headless backends; interaction assertions remain required.
    if let Ok(pixels) = window.take_snapshot() {
        require(
            pixels
                .as_bytes()
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] != 0),
        );
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/qwen-streaming-ui");
        let _ = std::fs::write(root.join(format!("{name}.rgba")), pixels.as_bytes());
        let _ = std::fs::write(
            root.join(format!("{name}.size")),
            format!("{} {}", pixels.width(), pixels.height()),
        );
    }
}
