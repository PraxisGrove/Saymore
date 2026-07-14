//! Home-page usage aggregation for presentation-layer callers.
//!
//! This API is additive: existing `HistoryStore` callers require no migration.

use chrono::{Duration, NaiveDate};

use crate::{HistoryCursor, HistoryRecord, HistoryStore, StorageError};

/// Number of rolling calendar days represented by the home-page usage trend.
pub const USAGE_TREND_DAYS: usize = 7;
const HISTORY_PAGE_SIZE: u16 = 50;

/// Aggregate usage data consumed by presentation-layer home pages.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsageSummary {
    /// Total recorded audio duration across retained history.
    pub total_duration_ms: u64,
    /// Total non-whitespace characters across retained final transcripts.
    pub total_characters: u64,
    /// Recorded duration for each day, ordered from oldest to newest.
    pub daily_duration_ms: [u64; USAGE_TREND_DAYS],
    /// Index of the first busiest day, or `None` when the trend is empty.
    pub highlighted_day: Option<usize>,
}

/// Loads persisted history and summarizes the cumulative and rolling seven-day usage.
pub fn load_usage_summary(
    store: &dyn HistoryStore,
    today: NaiveDate,
    date_for_timestamp: impl Fn(i64) -> Option<NaiveDate>,
) -> Result<UsageSummary, StorageError> {
    let records = load_all_history(store)?;
    Ok(summarize_history(&records, today, date_for_timestamp))
}

fn load_all_history(store: &dyn HistoryStore) -> Result<Vec<HistoryRecord>, StorageError> {
    let mut records = Vec::new();
    let mut cursor: Option<HistoryCursor> = None;
    loop {
        let page = store.history_page(cursor, HISTORY_PAGE_SIZE)?;
        records.extend(page.records);
        let Some(next_cursor) = page.next_cursor else {
            return Ok(records);
        };
        cursor = Some(next_cursor);
    }
}

fn summarize_history(
    records: &[HistoryRecord],
    today: NaiveDate,
    date_for_timestamp: impl Fn(i64) -> Option<NaiveDate>,
) -> UsageSummary {
    let total_duration_ms = records.iter().fold(0_u64, |total, record| {
        total.saturating_add(record.audio_duration_ms)
    });
    let total_characters = records.iter().fold(0_u64, |total, record| {
        let characters = record
            .final_text
            .chars()
            .filter(|character| !character.is_whitespace())
            .count() as u64;
        total.saturating_add(characters)
    });
    let period_start = today - Duration::days((USAGE_TREND_DAYS - 1) as i64);
    let mut daily_duration_ms = [0_u64; USAGE_TREND_DAYS];
    for record in records {
        let Some(date) = date_for_timestamp(record.created_at_ms) else {
            continue;
        };
        let day_index = date.signed_duration_since(period_start).num_days();
        if let Ok(day_index) = usize::try_from(day_index)
            && day_index < USAGE_TREND_DAYS
        {
            daily_duration_ms[day_index] =
                daily_duration_ms[day_index].saturating_add(record.audio_duration_ms);
        }
    }
    let maximum = daily_duration_ms.iter().copied().max().unwrap_or_default();
    let highlighted_day = daily_duration_ms
        .iter()
        .position(|duration| maximum > 0 && *duration == maximum);

    UsageSummary {
        total_duration_ms,
        total_characters,
        daily_duration_ms,
        highlighted_day,
    }
}

#[cfg(test)]
mod tests {
    use crate::{HistoryDelivery, HistoryRefinement};

    use super::*;

    #[test]
    fn summarizes_totals_and_the_rolling_seven_day_window() {
        let records = vec![
            record(1, "你好 世界", 60_000),
            record(2, "Saymore", 30_000),
            record(3, "旧记录", 60_000),
        ];

        assert_eq!(
            UsageSummary {
                total_duration_ms: 150_000,
                total_characters: 14,
                daily_duration_ms: [0, 0, 0, 0, 0, 60_000, 30_000],
                highlighted_day: Some(5),
            },
            summarize_history(
                &records,
                NaiveDate::from_ymd_opt(2026, 7, 15).unwrap_or_default(),
                |timestamp| match timestamp {
                    1 => NaiveDate::from_ymd_opt(2026, 7, 14),
                    2 => NaiveDate::from_ymd_opt(2026, 7, 15),
                    3 => NaiveDate::from_ymd_opt(2026, 7, 8),
                    _ => None,
                },
            )
        );
    }

    #[test]
    fn empty_history_has_zero_totals_and_no_highlight() {
        assert_eq!(
            UsageSummary {
                total_duration_ms: 0,
                total_characters: 0,
                daily_duration_ms: [0; USAGE_TREND_DAYS],
                highlighted_day: None,
            },
            summarize_history(
                &[],
                NaiveDate::from_ymd_opt(2026, 7, 15).unwrap_or_default(),
                |_| None
            )
        );
    }

    fn record(created_at_ms: i64, text: &str, duration_ms: u64) -> HistoryRecord {
        HistoryRecord {
            id: created_at_ms.to_string(),
            created_at_ms,
            final_text: text.to_owned(),
            raw_asr_text: None,
            llm_refined_text: None,
            audio_duration_ms: duration_ms,
            language: None,
            delivery: HistoryDelivery::Delivered,
            refinement: HistoryRefinement::NotUsed,
            asr_provider_id: None,
            llm_provider_id: None,
        }
    }
}
