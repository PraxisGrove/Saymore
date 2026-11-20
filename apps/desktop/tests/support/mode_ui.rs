use crate::ui;
#[path = "../../src/overlay_window.rs"]
mod overlay_window;
use slint::{ComponentHandle, platform::WindowEvent};
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
pub fn after_snapshot<C: ComponentHandle + 'static>(
    component: &C,
    directory: &'static str,
    name: &'static str,
    then: impl FnOnce() + 'static,
) {
    let weak = component.as_weak();
    // Let the reveal animation finish, then request a fresh frame for capture.
    slint::Timer::single_shot(Duration::from_millis(300), move || {
        let Some(component) = weak.upgrade() else {
            require(false);
            return;
        };
        let pending = Rc::new(RefCell::new(Some(then)));
        let observed = pending.clone();
        let notifier = component.window().set_rendering_notifier(move |state, _| {
            if matches!(state, slint::RenderingState::AfterRendering)
                && let Some(then) = observed.borrow_mut().take()
                && let Some(component) = weak.upgrade()
            {
                // FemtoVG reads the OpenGL back buffer. Capture it before the swap,
                // and dispatch input only after returning from the renderer.
                save_snapshot(component.window(), directory, name);
                slint::Timer::single_shot(Duration::ZERO, then);
            }
        });
        let unavailable = move || {
            if let Some(then) = pending.borrow_mut().take() {
                eprintln!("Snapshot unavailable; interaction assertions still run");
                then();
            }
        };
        match notifier {
            Ok(()) => {
                component.window().request_redraw();
                slint::Timer::single_shot(Duration::from_secs(5), unavailable);
            }
            Err(slint::SetRenderingNotifierError::Unsupported) => unavailable(),
            Err(_) => require(false),
        }
    });
}

pub fn show_preview_and_finish(overlay: &ui::RecordingOverlay, directory: &'static str) {
    // Match the production panel setup so an inactive test app remains visible.
    require(overlay_window::prepare(overlay.window()).is_ok());
    after_snapshot(overlay, directory, "streaming-preview", || {
        let _ = slint::quit_event_loop();
    });
    require(overlay_window::present(overlay.window()).is_ok());
    require(
        overlay
            .window()
            .try_dispatch_event(WindowEvent::Resized {
                size: slint::LogicalSize::new(360.0, 35.0),
            })
            .is_ok(),
    );
    overlay.window().request_redraw();
}

fn save_snapshot(window: &slint::Window, directory: &str, name: &str) {
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
