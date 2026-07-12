use slint::SharedString;
use template_app::RecordingMetrics;

use crate::{RecordingOverlay, format_duration, ui::AppWindow};

pub(crate) fn update(
    ui: &slint::Weak<AppWindow>,
    overlay: &slint::Weak<RecordingOverlay>,
    metrics: RecordingMetrics,
) {
    let metrics_overlay = overlay.clone();
    let _ = ui.upgrade_in_event_loop(move |ui| {
        ui.set_recording_level(metrics.level.clamp(0.0, 1.0));
        ui.set_recording_detail(SharedString::from(format!(
            "{} · {} 个输入采样",
            format_duration(metrics.elapsed_ms),
            metrics.input_sample_count
        )));
        if let Some(overlay) = metrics_overlay.upgrade() {
            overlay.set_recording_level(metrics.level.clamp(0.0, 1.0));
        }
    });
}
