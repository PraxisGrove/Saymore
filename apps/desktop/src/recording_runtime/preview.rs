use crate::{
    asr_runtime::AsrSessionController,
    ui::{AppWindow, RecordingOverlay},
};
use slint::{ComponentHandle, SharedString, Weak};
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
                overlay.set_preview_width(width);
                overlay
                    .window()
                    .set_size(slint::LogicalSize::new(width, 35.0));
            }
        });
    })
}

fn preview_width(text: &str) -> f32 {
    let growth = text.chars().count().min(27) as f32 * 6.0;
    (120.0 + growth).min(280.0)
}

pub(super) fn configure(overlay: &RecordingOverlay, asr: &AsrSessionController) {
    let preview = asr.shows_live_preview();
    overlay.set_preview_enabled(preview);
    overlay.set_preview_text(SharedString::default());
    overlay
        .window()
        .set_size(slint::LogicalSize::new(120.0, 35.0));
}
