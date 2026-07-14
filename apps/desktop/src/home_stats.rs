use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread,
};

use chrono::{Datelike, Duration, Local, NaiveDate, TimeZone, Weekday};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use template_app::{USAGE_TREND_DAYS, UsageSummary, load_usage_summary};
use template_infra::{
    SqliteStorage, open_accessibility_privacy_settings, open_microphone_privacy_settings,
};

use crate::ui::AppWindow;

pub fn wire(ui: &AppWindow, storage: Arc<SqliteStorage>) {
    let generation = Arc::new(AtomicU64::new(0));
    let refresh_ui = ui.as_weak();
    ui.on_refresh_usage(move || {
        refresh(
            refresh_ui.clone(),
            Arc::clone(&storage),
            Arc::clone(&generation),
        );
    });
    ui.on_open_microphone_settings(move || {
        if let Err(error) = open_microphone_privacy_settings() {
            tracing::warn!(event = "microphone.settings_open_failed", reason = %error);
        }
    });
    ui.on_open_accessibility_settings(move || {
        if let Err(error) = open_accessibility_privacy_settings() {
            tracing::warn!(event = "accessibility.settings_open_failed", reason = %error);
        }
    });
    ui.invoke_refresh_usage();
}

fn refresh(ui: slint::Weak<AppWindow>, storage: Arc<SqliteStorage>, generation: Arc<AtomicU64>) {
    let request_generation = generation.fetch_add(1, Ordering::Relaxed).saturating_add(1);
    if thread::Builder::new()
        .name("saymore-load-home-stats".to_owned())
        .spawn(move || {
            let today = Local::now().date_naive();
            let result = load_usage_summary(storage.as_ref(), today, |timestamp_ms| {
                Local
                    .timestamp_millis_opt(timestamp_ms)
                    .single()
                    .map(|timestamp| timestamp.date_naive())
            });
            if generation.load(Ordering::Relaxed) != request_generation {
                return;
            }
            let _ = ui.upgrade_in_event_loop(move |ui| match result {
                Ok(summary) => apply_summary(&ui, summary, today),
                Err(error) => {
                    tracing::warn!(event = "home.stats_load_failed", reason = %error);
                    apply_summary(&ui, UsageSummary::default(), today);
                }
            });
        })
        .is_err()
    {
        tracing::error!(event = "home.stats_worker_spawn_failed");
    }
}

fn apply_summary(ui: &AppWindow, summary: UsageSummary, today: NaiveDate) {
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
    let labels = day_labels(today)
        .into_iter()
        .map(SharedString::from)
        .collect::<Vec<_>>();

    ui.set_usage_total_minutes(SharedString::from(format_integer(total_minutes)));
    ui.set_usage_total_characters(SharedString::from(format_integer(summary.total_characters)));
    ui.set_usage_average_speed(SharedString::from(format_integer(average_speed)));
    ui.set_usage_trend(ModelRc::new(VecModel::from(trend.to_vec())));
    ui.set_usage_day_labels(ModelRc::new(VecModel::from(labels)));
    ui.set_usage_highlighted_day(summary.highlighted_day.map_or(-1, |index| index as i32));
}

fn day_labels(today: NaiveDate) -> [&'static str; USAGE_TREND_DAYS] {
    std::array::from_fn(|index| {
        let days_ago = (USAGE_TREND_DAYS - index - 1) as i64;
        match (today - Duration::days(days_ago)).weekday() {
            Weekday::Mon => "一",
            Weekday::Tue => "二",
            Weekday::Wed => "三",
            Weekday::Thu => "四",
            Weekday::Fri => "五",
            Weekday::Sat => "六",
            Weekday::Sun => "日",
        }
    })
}

fn format_integer(value: u64) -> String {
    let digits = value.to_string();
    let mut formatted = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            formatted.push(',');
        }
        formatted.push(digit);
    }
    formatted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_cover_the_rolling_seven_day_window() {
        assert_eq!(
            ["四", "五", "六", "日", "一", "二", "三"],
            day_labels(NaiveDate::from_ymd_opt(2026, 7, 15).unwrap_or_default())
        );
    }

    #[test]
    fn integer_formatting_uses_group_separators() {
        assert_eq!("0", format_integer(0));
        assert_eq!("999", format_integer(999));
        assert_eq!("6,240", format_integer(6_240));
        assert_eq!("1,000,000", format_integer(1_000_000));
    }
}
