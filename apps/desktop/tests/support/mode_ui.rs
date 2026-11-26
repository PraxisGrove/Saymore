use crate::ui;
#[path = "../../src/overlay_window.rs"]
mod overlay_window;
#[path = "../../src/recording_runtime/preview.rs"]
mod preview;
use slint::ComponentHandle;
use std::{cell::RefCell, rc::Rc, sync::Arc, time::Duration};
use template_app::{DictationSession, DictationSessionId, DictationToggleAction};

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

pub fn show_preview_and_finish(
    window: &ui::AppWindow,
    overlay: &ui::RecordingOverlay,
    directory: &'static str,
) {
    let window = window.clone_strong();
    let overlay = overlay.clone_strong();
    require(
        slint::spawn_local(async move {
            let session = Arc::new(DictationSession::default());
            let waveform = start_preview(&session, &overlay, preview::PreviewMode::WaveformOnly);
            preview::callback(
                window.as_weak(),
                overlay.as_weak(),
                session.clone(),
                waveform,
            )("main-window progress only".to_owned());
            after_partials().await;
            check_preview(&overlay, "", 120.0);
            require(window.get_recording_detail() == "main-window progress only");
            session.complete();

            let first = start_preview(&session, &overlay, preview::PreviewMode::LiveTranscript);
            check_preview(&overlay, "", 120.0);
            require(overlay_window::prepare(overlay.window()).is_ok());
            overlay.set_revealed(true);
            require(overlay_window::present(overlay.window()).is_ok());
            let partial =
                preview::callback(window.as_weak(), overlay.as_weak(), session.clone(), first);
            partial("hello".to_owned());
            after_partials().await;
            check_preview(&overlay, "hello", 150.0);

            require(session.request_finish() == Some(first));
            preview::show_processing(&overlay, "Transcribing".into());
            partial("late transcript after stop".to_owned());
            after_partials().await;
            require(overlay.get_mode() == 1);
            check_preview(&overlay, "hello", 150.0);

            session.complete();
            partial("old transcript queued before restarting".to_owned());
            let second = start_preview(&session, &overlay, preview::PreviewMode::LiveTranscript);
            after_partials().await;
            require(overlay.get_mode() == 0);
            check_preview(&overlay, "", 120.0);
            require(window.get_recording_detail() == "hello");

            let current =
                preview::callback(window.as_weak(), overlay.as_weak(), session.clone(), second);
            current("这是流式识别预览。\n The text updates while you speak.".to_owned());
            after_partials().await;
            check_preview(
                &overlay,
                concat!("这是流式识别预览。", " The text updates while you speak."),
                280.0,
            );

            overlay.set_revealed(true);
            let displayed = overlay.as_weak();
            after_snapshot(&overlay, directory, "streaming-preview", move || {
                if let Some(displayed) = displayed.upgrade() {
                    let window = displayed.window();
                    let expected =
                        slint::LogicalSize::new(280.0, 35.0).to_physical(window.scale_factor());
                    require(window.size() == expected);
                } else {
                    require(false);
                }
                let _ = slint::quit_event_loop();
            });
        })
        .is_ok(),
    );
}

fn start_preview(
    session: &DictationSession,
    overlay: &ui::RecordingOverlay,
    mode: preview::PreviewMode,
) -> DictationSessionId {
    let DictationToggleAction::Start(id) = session.request_toggle(false) else {
        std::process::exit(1);
    };
    require(session.recording_started());
    preview::begin(overlay, mode);
    id
}

fn check_preview(overlay: &ui::RecordingOverlay, text: &str, width: f32) {
    require(overlay.get_preview_text() == text);
    require((overlay.get_preview_width() - width).abs() < f32::EPSILON);
}

async fn after_partials() {
    let (sent, received) = tokio::sync::oneshot::channel();
    require(
        slint::invoke_from_event_loop(move || {
            let _ = sent.send(());
        })
        .is_ok(),
    );
    require(received.await.is_ok());
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
