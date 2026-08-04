use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread,
};

use chrono::{Datelike, Duration, Local, NaiveDate};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use template_app::{USAGE_TREND_DAYS, UsageSummary, load_usage_summary};
use template_infra::{SqliteStorage, local_storage_usage};

use crate::{regional_format, storage_usage_ui, ui::AppWindow};

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
    ui.invoke_refresh_usage();
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
            let result = load_usage_summary(storage.as_ref(), today);
            let storage_usage = local_storage_usage(&data_directory);
            if generation.load(Ordering::Relaxed) != request_generation {
                return;
            }
            let _ = ui.upgrade_in_event_loop(move |ui| {
                match storage_usage {
                    Ok(usage) => {
                        storage_usage_ui::apply(&ui, usage, system_locale.as_deref());
                    }
                    Err(error) => {
                        tracing::warn!(event = "storage.usage_load_failed", reason = %error);
                        storage_usage_ui::apply_error(&ui);
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
                        ui.set_usage_day_labels(ModelRc::default());
                        ui.set_usage_day_dates(ModelRc::default());
                        ui.set_usage_daily_minutes(ModelRc::default());
                        ui.set_usage_daily_characters(ModelRc::default());
                        ui.set_usage_daily_speeds(ModelRc::default());
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
    let total_minutes = rounded_minutes(summary.total_duration_ms);
    let overall_average_speed = average_speed(summary.total_characters, summary.total_duration_ms);
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
    let dates = day_dates(today, system_locale)
        .into_iter()
        .map(SharedString::from)
        .collect::<Vec<_>>();
    let daily_minutes = summary.daily_duration_ms.map(rounded_minutes);
    let daily_speeds = std::array::from_fn(|index| {
        average_speed(
            summary.daily_characters[index],
            summary.daily_duration_ms[index],
        )
    });

    ui.set_usage_total_minutes(SharedString::from(regional_format::format_integer(
        total_minutes,
        system_locale,
    )));
    ui.set_usage_total_characters(SharedString::from(regional_format::format_integer(
        summary.total_characters,
        system_locale,
    )));
    ui.set_usage_average_speed(SharedString::from(regional_format::format_integer(
        overall_average_speed,
        system_locale,
    )));
    ui.set_usage_trend(ModelRc::new(VecModel::from(trend.to_vec())));
    ui.set_usage_day_dates(ModelRc::new(VecModel::from(dates)));
    ui.set_usage_daily_minutes(ModelRc::new(VecModel::from(format_values(
        daily_minutes,
        system_locale,
    ))));
    ui.set_usage_daily_characters(ModelRc::new(VecModel::from(format_values(
        summary.daily_characters,
        system_locale,
    ))));
    ui.set_usage_daily_speeds(ModelRc::new(VecModel::from(format_values(
        daily_speeds,
        system_locale,
    ))));
    ui.set_usage_day_labels(ModelRc::new(VecModel::from(labels)));
    ui.set_usage_highlighted_day(summary.highlighted_day.map_or(-1, |index| index as i32));
}

fn rounded_minutes(duration_ms: u64) -> u64 {
    duration_ms.saturating_add(30_000) / 60_000
}

fn average_speed(characters: u64, duration_ms: u64) -> u64 {
    if duration_ms == 0 {
        return 0;
    }
    let value = u128::from(characters)
        .saturating_mul(60_000)
        .saturating_add(u128::from(duration_ms / 2))
        / u128::from(duration_ms);
    value.min(u128::from(u64::MAX)) as u64
}

fn format_values(
    values: [u64; USAGE_TREND_DAYS],
    system_locale: Option<&str>,
) -> Vec<SharedString> {
    values
        .map(|value| SharedString::from(regional_format::format_integer(value, system_locale)))
        .to_vec()
}

fn day_labels(today: NaiveDate, system_locale: Option<&str>) -> [String; USAGE_TREND_DAYS] {
    let locale = regional_format::date_locale(system_locale);
    let chinese = system_locale.is_some_and(|value| value.to_ascii_lowercase().starts_with("zh"));
    std::array::from_fn(|index| {
        let days_ago = (USAGE_TREND_DAYS - index - 1) as i64;
        let label = (today - Duration::days(days_ago))
            .format_localized("%a", locale)
            .to_string();
        if chinese {
            format!("周{label}")
        } else {
            label
        }
    })
}

fn day_dates(today: NaiveDate, system_locale: Option<&str>) -> [String; USAGE_TREND_DAYS] {
    let locale = regional_format::date_locale(system_locale);
    let chinese = system_locale.is_some_and(|value| value.to_ascii_lowercase().starts_with("zh"));
    std::array::from_fn(|index| {
        let days_ago = (USAGE_TREND_DAYS - index - 1) as i64;
        let date = today - Duration::days(days_ago);
        if chinese {
            format!("{} 月 {} 日", date.month(), date.day())
        } else {
            date.format_localized("%b %-d", locale).to_string()
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_cover_the_rolling_seven_day_window() {
        assert_eq!(
            ["周四", "周五", "周六", "周日", "周一", "周二", "周三"],
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
    fn dates_cover_the_same_rolling_window() {
        assert_eq!(
            [
                "7 月 9 日",
                "7 月 10 日",
                "7 月 11 日",
                "7 月 12 日",
                "7 月 13 日",
                "7 月 14 日",
                "7 月 15 日"
            ],
            day_dates(
                NaiveDate::from_ymd_opt(2026, 7, 15).unwrap_or_default(),
                Some("zh-CN")
            )
        );
    }
}
