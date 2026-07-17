use slint::ComponentHandle;
use template_app::RecordingMetrics;

use crate::{
    ui::{AppWindow, RecordingOverlay, Translations},
    ui_status::format_duration,
};

pub(crate) fn update(
    ui: &slint::Weak<AppWindow>,
    overlay: &slint::Weak<RecordingOverlay>,
    metrics: RecordingMetrics,
) {
    let metrics_overlay = overlay.clone();
    let _ = ui.upgrade_in_event_loop(move |ui| {
        ui.set_recording_level(metrics.level.clamp(0.0, 1.0));
        ui.set_recording_detail(ui.global::<Translations>().invoke_recording_samples(
            format_duration(metrics.elapsed_ms).into(),
            metrics.input_sample_count.try_into().unwrap_or(i32::MAX),
        ));
        if let Some(overlay) = metrics_overlay.upgrade() {
            overlay.set_recording_level(metrics.level.clamp(0.0, 1.0));
        }
    });
}
