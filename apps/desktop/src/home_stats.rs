use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread,
};

use chrono::{Duration, Local, NaiveDate, TimeZone};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use template_app::{USAGE_TREND_DAYS, UsageSummary, load_usage_summary};
use template_infra::{SqliteStorage, directory_usage_bytes};

use crate::{
    regional_format,
    ui::{AppWindow, Translations},
};

pub fn wire(ui: &AppWindow, storage: Arc<SqliteStorage>, data_directory: PathBuf) {
    let generation = Arc::new(AtomicU64::new(0));
    let refresh_ui = ui.as_weak();
    ui.on_refresh_usage(move || {
        refresh(
            refresh_ui.clone(),
            Arc::clone(&storage),
            data_directory.clone(),
            Arc::clone(&generation),
        );
    });
    ui.on_open_microphone_settings(move || {
        if let Err(error) = open_microphone_settings() {
            tracing::warn!(event = "microphone.settings_open_failed", reason = %error);
        }
    });
    ui.on_open_accessibility_settings(move || {
        if let Err(error) = open_accessibility_settings() {
            tracing::warn!(event = "accessibility.settings_open_failed", reason = %error);
        }
    });
    ui.invoke_refresh_usage();
}

#[cfg(target_os = "macos")]
fn open_microphone_settings() -> Result<(), String> {
    template_infra::open_microphone_privacy_settings().map_err(|error| error.to_string())
}

#[cfg(target_os = "windows")]
fn open_microphone_settings() -> Result<(), String> {
    template_infra::open_windows_microphone_privacy_settings().map_err(|error| error.to_string())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn open_microphone_settings() -> Result<(), String> {
    Err("microphone settings integration is not available on this platform yet".to_owned())
}

#[cfg(target_os = "macos")]
fn open_accessibility_settings() -> Result<(), String> {
    template_infra::open_accessibility_privacy_settings().map_err(|error| error.to_string())
}

#[cfg(not(target_os = "macos"))]
fn open_accessibility_settings() -> Result<(), String> {
    Err("accessibility settings integration is not available on this platform yet".to_owned())
}

fn refresh(
    ui: slint::Weak<AppWindow>,
    storage: Arc<SqliteStorage>,
    data_directory: PathBuf,
    generation: Arc<AtomicU64>,
) {
    let request_generation = generation.fetch_add(1, Ordering::Relaxed).saturating_add(1);
    if let Some(ui) = ui.upgrade() {
        ui.set_usage_loading(true);
        ui.set_usage_error(false);
    }
    let failure_ui = ui.clone();
    if thread::Builder::new()
        .name("saymore-load-home-stats".to_owned())
        .spawn(move || {
            let today = Local::now().date_naive();
            let system_locale = regional_format::system_locale();
            let result = load_usage_summary(storage.as_ref(), today, |timestamp_ms| {
                Local
                    .timestamp_millis_opt(timestamp_ms)
                    .single()
                    .map(|timestamp| timestamp.date_naive())
            });
            let storage_usage = directory_usage_bytes(&data_directory);
            if generation.load(Ordering::Relaxed) != request_generation {
                return;
            }
            let _ = ui.upgrade_in_event_loop(move |ui| {
                match storage_usage {
                    Ok(bytes) => ui.set_storage_usage(SharedString::from(format_storage_usage(
                        bytes,
                        system_locale.as_deref(),
                    ))),
                    Err(error) => {
                        tracing::warn!(event = "storage.usage_load_failed", reason = %error);
                        ui.set_storage_usage(ui.global::<Translations>().get_storage_unavailable());
                    }
                }
                match result {
                    Ok(summary) => {
                        apply_summary(&ui, summary, today, system_locale.as_deref());
                        ui.set_usage_loading(false);
                        ui.set_usage_error(false);
                    }
                    Err(error) => {
                        tracing::warn!(event = "home.stats_load_failed", reason = %error);
                        ui.set_usage_loading(false);
                        ui.set_usage_error(true);
                        ui.set_usage_trend(ModelRc::default());
                    }
                }
            });
        })
        .is_err()
    {
        tracing::error!(event = "home.stats_worker_spawn_failed");
        let _ = failure_ui.upgrade_in_event_loop(|ui| {
            ui.set_usage_loading(false);
            ui.set_usage_error(true);
        });
    }
}

fn apply_summary(
    ui: &AppWindow,
    summary: UsageSummary,
    today: NaiveDate,
    system_locale: Option<&str>,
) {
    let total_minutes = summary.total_duration_ms.saturating_add(30_000) / 60_000;
    let average_speed = if summary.total_duration_ms == 0 {
        0
    } else {
        let value = u128::from(summary.total_characters)
            .saturating_mul(60_000)
            .saturating_add(u128::from(summary.total_duration_ms / 2))
            / u128::from(summary.total_duration_ms);
        value.min(u128::from(u64::MAX)) as u64
    };
    let maximum = summary
        .daily_duration_ms
        .iter()
        .copied()
        .max()
        .unwrap_or_default();
    let trend = summary.daily_duration_ms.map(|duration| {
        if maximum == 0 {
            0.0
        } else {
            duration as f32 / maximum as f32
        }
    });
    let labels = day_labels(today, system_locale)
        .into_iter()
        .map(SharedString::from)
        .collect::<Vec<_>>();

    ui.set_usage_total_minutes(SharedString::from(regional_format::format_integer(
        total_minutes,
        system_locale,
    )));
    ui.set_usage_total_characters(SharedString::from(regional_format::format_integer(
        summary.total_characters,
        system_locale,
    )));
    ui.set_usage_average_speed(SharedString::from(regional_format::format_integer(
        average_speed,
        system_locale,
    )));
    ui.set_usage_trend(ModelRc::new(VecModel::from(trend.to_vec())));
    ui.set_usage_day_labels(ModelRc::new(VecModel::from(labels)));
    ui.set_usage_highlighted_day(summary.highlighted_day.map_or(-1, |index| index as i32));
}

fn day_labels(today: NaiveDate, system_locale: Option<&str>) -> [String; USAGE_TREND_DAYS] {
    let locale = regional_format::date_locale(system_locale);
    std::array::from_fn(|index| {
        let days_ago = (USAGE_TREND_DAYS - index - 1) as i64;
        (today - Duration::days(days_ago))
            .format_localized("%a", locale)
            .to_string()
    })
}

fn format_storage_usage(bytes: u64, system_locale: Option<&str>) -> String {
    const KIB: u64 = 1_024;
    const MIB: u64 = KIB * 1_024;
    const GIB: u64 = MIB * 1_024;

    if bytes == 0 {
        return "0 MB".to_owned();
    }
    if bytes < MIB {
        return format_decimal(bytes as f64 / KIB as f64, "KB", system_locale);
    }
    if bytes < GIB {
        return format_decimal(bytes as f64 / MIB as f64, "MB", system_locale);
    }
    format_decimal(bytes as f64 / GIB as f64, "GB", system_locale)
}

fn format_decimal(value: f64, unit: &str, system_locale: Option<&str>) -> String {
    let value =
        format!("{value:.1}").replace('.', regional_format::decimal_separator(system_locale));
    format!("{value} {unit}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_cover_the_rolling_seven_day_window() {
        assert_eq!(
            ["四", "五", "六", "日", "一", "二", "三"],
            day_labels(
                NaiveDate::from_ymd_opt(2026, 7, 15).unwrap_or_default(),
                Some("zh-CN")
            )
        );
        assert_eq!(
            ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"],
            day_labels(
                NaiveDate::from_ymd_opt(2026, 7, 15).unwrap_or_default(),
                Some("en-US")
            )
        );
    }

    #[test]
    fn storage_usage_uses_readable_units() {
        assert_eq!("0 MB", format_storage_usage(0, Some("en-US")));
        assert_eq!("1.5 KB", format_storage_usage(1_536, Some("en-US")));
        assert_eq!("1,5 MB", format_storage_usage(1_572_864, Some("de-DE")));
    }
}
