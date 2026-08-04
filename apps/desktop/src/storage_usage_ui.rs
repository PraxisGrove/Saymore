use slint::{ComponentHandle, SharedString};
use template_app::LocalStorageUsage;

use crate::{
    regional_format,
    ui::{AppWindow, Translations},
};

pub(crate) fn apply(ui: &AppWindow, usage: LocalStorageUsage, system_locale: Option<&str>) {
    let raw_shares = shares(
        usage.total_bytes,
        [
            usage.local_models_bytes,
            usage.recognition_data_bytes,
            usage.diagnostic_logs_bytes,
            usage.configuration_other_bytes,
        ],
    );
    let visual_shares = exaggerated_shares(raw_shares);
    let ring_paths = ring_paths(visual_shares);
    let model_total = usage.models.total_bytes();
    let model_shares = shares(
        model_total,
        [
            usage.models.qwen3_asr_bytes,
            usage.models.sense_voice_bytes,
            usage.models.whisper_bytes,
            usage.models.paraformer_bytes,
            usage.models.punctuation_bytes,
        ],
    );

    ui.set_storage_usage(format_bytes(usage.total_bytes, system_locale).into());
    ui.set_storage_local_models_usage(format_bytes(usage.local_models_bytes, system_locale).into());
    ui.set_storage_recognition_usage(
        format_bytes(usage.recognition_data_bytes, system_locale).into(),
    );
    ui.set_storage_diagnostics_usage(
        format_bytes(usage.diagnostic_logs_bytes, system_locale).into(),
    );
    ui.set_storage_other_usage(format_bytes(usage.configuration_other_bytes, system_locale).into());
    ui.set_storage_local_models_percent(format_percent(raw_shares[0]));
    ui.set_storage_recognition_percent(format_percent(raw_shares[1]));
    ui.set_storage_diagnostics_percent(format_percent(raw_shares[2]));
    ui.set_storage_other_percent(format_percent(raw_shares[3]));
    ui.set_storage_local_models_visual_share(visual_shares[0]);
    ui.set_storage_recognition_visual_share(visual_shares[1]);
    ui.set_storage_diagnostics_visual_share(visual_shares[2]);
    ui.set_storage_other_visual_share(visual_shares[3]);
    ui.set_storage_local_models_path(ring_paths[0].clone());
    ui.set_storage_recognition_path(ring_paths[1].clone());
    ui.set_storage_diagnostics_path(ring_paths[2].clone());
    ui.set_storage_other_path(ring_paths[3].clone());
    ui.set_local_models_total_usage(format_bytes(model_total, system_locale).into());
    ui.set_qwen3_storage_usage(format_bytes(usage.models.qwen3_asr_bytes, system_locale).into());
    ui.set_sense_voice_storage_usage(
        format_bytes(usage.models.sense_voice_bytes, system_locale).into(),
    );
    ui.set_whisper_storage_usage(format_bytes(usage.models.whisper_bytes, system_locale).into());
    ui.set_paraformer_storage_usage(
        format_bytes(usage.models.paraformer_bytes, system_locale).into(),
    );
    ui.set_punctuation_storage_usage(
        format_bytes(usage.models.punctuation_bytes, system_locale).into(),
    );
    ui.set_qwen3_model_share(model_shares[0]);
    ui.set_sense_voice_model_share(model_shares[1]);
    ui.set_whisper_model_share(model_shares[2]);
    ui.set_paraformer_model_share(model_shares[3]);
    ui.set_punctuation_model_share(model_shares[4]);
    ui.set_storage_usage_error(false);
}

pub(crate) fn apply_error(ui: &AppWindow) {
    ui.set_storage_usage(ui.global::<Translations>().get_storage_unavailable());
    ui.set_storage_usage_error(true);
}

pub(crate) fn format_bytes(bytes: u64, system_locale: Option<&str>) -> String {
    const KIB: u64 = 1_024;
    const MIB: u64 = KIB * 1_024;
    const GIB: u64 = MIB * 1_024;

    if bytes == 0 {
        return "0 MB".to_owned();
    }
    if bytes < MIB {
        return format_decimal(bytes as f64 / KIB as f64, "KB", system_locale, 1);
    }
    if bytes < GIB {
        return format_decimal(bytes as f64 / MIB as f64, "MB", system_locale, 1);
    }
    format_decimal(bytes as f64 / GIB as f64, "GB", system_locale, 2)
}

fn format_decimal(value: f64, unit: &str, system_locale: Option<&str>, precision: usize) -> String {
    let value = format!("{value:.precision$}")
        .replace('.', regional_format::decimal_separator(system_locale));
    format!("{value} {unit}")
}

fn shares<const N: usize>(total: u64, values: [u64; N]) -> [f32; N] {
    if total == 0 {
        return [0.0; N];
    }
    values.map(|value| value as f32 / total as f32)
}

fn exaggerated_shares(raw: [f32; 4]) -> [f32; 4] {
    // Compress the range while preserving category order, so tiny categories
    // remain visible without turning every small segment into the same size.
    let adjusted = raw.map(f32::sqrt);
    let total = adjusted.iter().sum::<f32>();
    if total == 0.0 {
        return adjusted;
    }
    adjusted.map(|share| share / total)
}

fn format_percent(share: f32) -> SharedString {
    if share > 0.0 && share < 0.001 {
        return "<0.1%".into();
    }
    format!("{:.1}%", share * 100.0).into()
}

fn ring_paths(shares: [f32; 4]) -> [SharedString; 4] {
    let mut start_degrees = -90.0_f32;
    shares.map(|share| {
        let sweep = share * 360.0;
        let gap = 2.4_f32.min(sweep * 0.35);
        let path =
            rounded_ring_segment(start_degrees + gap / 2.0, start_degrees + sweep - gap / 2.0);
        start_degrees += sweep;
        path.into()
    })
}

fn rounded_ring_segment(start_degrees: f32, end_degrees: f32) -> String {
    if end_degrees <= start_degrees {
        return String::new();
    }
    const CENTER: f32 = 100.0;
    const OUTER_RADIUS: f32 = 92.0;
    const INNER_RADIUS: f32 = 64.0;
    const MAX_CORNER_RADIUS: f32 = 3.0;

    let sweep = end_degrees - start_degrees;
    let corner_radius = MAX_CORNER_RADIUS.min(sweep.to_radians() * INNER_RADIUS * 0.22);
    let outer_corner_degrees = (corner_radius / OUTER_RADIUS).to_degrees();
    let inner_corner_degrees = (corner_radius / INNER_RADIUS).to_degrees();

    let outer_start = point_on_circle(start_degrees + outer_corner_degrees, CENTER, OUTER_RADIUS);
    let outer_end = point_on_circle(end_degrees - outer_corner_degrees, CENTER, OUTER_RADIUS);
    let end_outer_corner = point_on_circle(end_degrees, CENTER, OUTER_RADIUS);
    let end_outer_edge = point_on_circle(end_degrees, CENTER, OUTER_RADIUS - corner_radius);
    let end_inner_edge = point_on_circle(end_degrees, CENTER, INNER_RADIUS + corner_radius);
    let end_inner_corner = point_on_circle(end_degrees, CENTER, INNER_RADIUS);
    let inner_end = point_on_circle(end_degrees - inner_corner_degrees, CENTER, INNER_RADIUS);
    let inner_start = point_on_circle(start_degrees + inner_corner_degrees, CENTER, INNER_RADIUS);
    let start_inner_corner = point_on_circle(start_degrees, CENTER, INNER_RADIUS);
    let start_inner_edge = point_on_circle(start_degrees, CENTER, INNER_RADIUS + corner_radius);
    let start_outer_edge = point_on_circle(start_degrees, CENTER, OUTER_RADIUS - corner_radius);
    let start_outer_corner = point_on_circle(start_degrees, CENTER, OUTER_RADIUS);
    let large_arc = i32::from(
        end_degrees - outer_corner_degrees - start_degrees - outer_corner_degrees > 180.0,
    );

    format!(
        "M {:.3} {:.3} A {OUTER_RADIUS} {OUTER_RADIUS} 0 {large_arc} 1 {:.3} {:.3} Q {:.3} {:.3} {:.3} {:.3} L {:.3} {:.3} Q {:.3} {:.3} {:.3} {:.3} A {INNER_RADIUS} {INNER_RADIUS} 0 {large_arc} 0 {:.3} {:.3} Q {:.3} {:.3} {:.3} {:.3} L {:.3} {:.3} Q {:.3} {:.3} {:.3} {:.3} Z",
        outer_start.0,
        outer_start.1,
        outer_end.0,
        outer_end.1,
        end_outer_corner.0,
        end_outer_corner.1,
        end_outer_edge.0,
        end_outer_edge.1,
        end_inner_edge.0,
        end_inner_edge.1,
        end_inner_corner.0,
        end_inner_corner.1,
        inner_end.0,
        inner_end.1,
        inner_start.0,
        inner_start.1,
        start_inner_corner.0,
        start_inner_corner.1,
        start_inner_edge.0,
        start_inner_edge.1,
        start_outer_edge.0,
        start_outer_edge.1,
        start_outer_corner.0,
        start_outer_corner.1,
        outer_start.0,
        outer_start.1,
    )
}

fn point_on_circle(degrees: f32, center: f32, radius: f32) -> (f32, f32) {
    let radians = degrees.to_radians();
    (
        center + radius * radians.cos(),
        center + radius * radians.sin(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_usage_uses_readable_units() {
        assert_eq!("0 MB", format_bytes(0, Some("en-US")));
        assert_eq!("1.5 KB", format_bytes(1_536, Some("en-US")));
        assert_eq!("1,5 MB", format_bytes(1_572_864, Some("de-DE")));
        assert_eq!("3.43 GB", format_bytes(3_678_363_084, Some("en-US")));
    }

    #[test]
    fn small_nonzero_categories_remain_visible_without_changing_real_percentages() {
        let displayed = exaggerated_shares([0.991, 0.007, 0.002, 0.0001]);

        assert!(displayed[1] > displayed[2]);
        assert!(displayed[2] > displayed[3]);
        assert!(displayed[3] > 0.005);
        assert!((displayed.iter().sum::<f32>() - 1.0).abs() < f32::EPSILON * 8.0);
        assert_eq!("0.7%", format_percent(0.007).as_str());
        assert_eq!("<0.1%", format_percent(0.0001).as_str());
    }

    #[test]
    fn ring_paths_follow_the_exaggerated_category_order() {
        let paths = ring_paths([0.8, 0.1, 0.06, 0.04]);

        assert!(paths.iter().all(|path| path.starts_with("M ")));
        assert!(paths.iter().all(|path| path.ends_with(" Z")));
        assert!(paths.iter().all(|path| path.contains(" Q ")));
        assert!(paths[0].contains(" A 92 92 0 1 1 "));
        assert!(paths[0].contains(" A 64 64 0 1 0 "));
        assert!(paths[1].contains(" A 92 92 0 0 1 "));
        assert!(paths[1].contains(" A 64 64 0 0 0 "));
    }
}
