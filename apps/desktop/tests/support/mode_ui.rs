use crate::ui;
#[path = "../../src/overlay_window.rs"]
mod overlay_window;
use slint::{
    ComponentHandle,
    platform::{PointerEventButton, WindowEvent},
};
use std::{cell::RefCell, rc::Rc, time::Duration};

#[track_caller]
pub fn require(valid: bool) {
    if !valid {
        eprintln!(
            "Recognition mode UI contract failed at {}",
            std::panic::Location::caller()
        );
        std::process::exit(1);
    }
}
pub fn click(ui: &ui::AppWindow, x: f32, y: f32) {
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

pub fn click_mode(ui: &ui::AppWindow, mode: ui::LocalRecognitionMode) {
    // The modal is centered; native desktops can clamp the requested window height.
    let size = ui.window().size().to_logical(ui.window().scale_factor());
    let x = match mode {
        ui::LocalRecognitionMode::Streaming => 690.0,
        ui::LocalRecognitionMode::NonStreaming => 590.0,
    };
    click(
        ui,
        x + (size.width - 920.0) / 2.0,
        320.0 + (size.height - 700.0) / 2.0,
    );
}

pub enum FrameStatus {
    Rendered,
    Unavailable,
}

pub fn after_frame(window: &slint::Window, then: impl FnOnce(FrameStatus) + 'static) {
    let pending = Rc::new(RefCell::new(Some(then)));
    let observed = pending.clone();
    let notifier = window.set_rendering_notifier(move |state, _| {
        if matches!(state, slint::RenderingState::AfterRendering) {
            complete(&observed, FrameStatus::Rendered);
        }
    });
    match notifier {
        Ok(()) => slint::Timer::single_shot(Duration::from_secs(5), move || {
            // Locked or occluded desktops may never draw, even with a valid native window.
            complete(&pending, FrameStatus::Unavailable);
        }),
        Err(slint::SetRenderingNotifierError::Unsupported) => {
            complete(&pending, FrameStatus::Unavailable);
        }
        Err(_) => require(false),
    }
    window.request_redraw();
}

fn complete<F: FnOnce(FrameStatus) + 'static>(pending: &RefCell<Option<F>>, state: FrameStatus) {
    if let Some(then) = pending.borrow_mut().take() {
        if matches!(state, FrameStatus::Unavailable) {
            eprintln!(
                "Snapshot unavailable: the backend did not produce a new frame; interaction assertions still run"
            );
        }
        // Leave the renderer and allow the 110/160 ms reveal animations to settle.
        let delay = match state {
            FrameStatus::Rendered => Duration::from_millis(200),
            FrameStatus::Unavailable => Duration::ZERO,
        };
        slint::Timer::single_shot(delay, move || then(state));
    }
}

pub fn show_preview_and_finish(overlay: &ui::RecordingOverlay, directory: &'static str) {
    // Match the production panel setup so an inactive test app remains visible.
    require(overlay_window::prepare(overlay.window()).is_ok());
    let weak = overlay.as_weak();
    after_frame(overlay.window(), move |frame| {
        if let Some(overlay) = weak.upgrade()
            && matches!(frame, FrameStatus::Rendered)
        {
            save_snapshot(overlay.window(), directory, "streaming-preview");
        }
        let _ = slint::quit_event_loop();
    });
    require(overlay_window::present(overlay.window()).is_ok());
    require(
        overlay
            .window()
            .try_dispatch_event(WindowEvent::Resized {
                size: slint::LogicalSize::new(360.0, 128.0),
            })
            .is_ok(),
    );
    overlay.window().request_redraw();
}

pub fn save_snapshot(window: &slint::Window, directory: &str, name: &str) {
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
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target")
            .join(directory);
        let _ = std::fs::write(root.join(format!("{name}.rgba")), pixels.as_bytes());
        let _ = std::fs::write(
            root.join(format!("{name}.size")),
            format!("{} {}", pixels.width(), pixels.height()),
        );
    }
}
