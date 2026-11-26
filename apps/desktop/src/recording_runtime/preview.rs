use crate::ui::{AppWindow, RecordingOverlay};
use slint::{ComponentHandle, LogicalSize, SharedString, Weak};
use std::sync::Arc;
use template_app::{DictationSession, DictationSessionId};

pub(super) fn callback(
    ui: Weak<AppWindow>,
    overlay: Weak<RecordingOverlay>,
    session: Arc<DictationSession>,
    id: DictationSessionId,
) -> Arc<dyn Fn(String) + Send + Sync> {
    Arc::new(move |text| {
        let session = Arc::clone(&session);
        let overlay = overlay.clone();
        let _ = ui.upgrade_in_event_loop(move |ui| {
            if session.current_id() != Some(id) || !session.is_recording() {
                return;
            }
            ui.set_recording_detail(SharedString::from(text.clone()));
            if let Some(overlay) = overlay.upgrade()
                && overlay.get_preview_enabled()
            {
                let width = preview_width(&text);
                overlay
                    .set_preview_text(text.split_whitespace().collect::<Vec<_>>().join(" ").into());
                resize(&overlay, width);
            }
        });
    })
}

const COMPACT_WIDTH: f32 = 120.0;
const MAX_WIDTH: f32 = 280.0;
const HEIGHT: f32 = 35.0;

pub(crate) enum PreviewMode {
    WaveformOnly,
    LiveTranscript,
}

pub(crate) fn begin(overlay: &RecordingOverlay, mode: PreviewMode) -> i32 {
    let generation = overlay.get_session_generation().wrapping_add(1);
    overlay.set_session_generation(generation);
    overlay.set_mode(0);
    overlay.set_recording_level(0.0);
    overlay.set_revealed(false);
    overlay.set_preview_enabled(matches!(mode, PreviewMode::LiveTranscript));
    overlay.set_preview_text(SharedString::default());
    resize(overlay, COMPACT_WIDTH);
    generation
}

pub(crate) fn show_processing(overlay: &RecordingOverlay, label: SharedString) -> i32 {
    // Keep the last accepted draft and its width until the next recording.
    overlay.set_mode(1);
    overlay.set_processing_label(label);
    overlay.get_session_generation()
}

fn preview_width(text: &str) -> f32 {
    let growth = text.chars().count().min(27) as f32 * 6.0;
    (COMPACT_WIDTH + growth).min(MAX_WIDTH)
}

fn resize(overlay: &RecordingOverlay, width: f32) {
    overlay.set_preview_width(width);
    overlay.window().set_size(LogicalSize::new(width, HEIGHT));
}
