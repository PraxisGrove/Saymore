//! Home-page usage aggregation for presentation-layer callers.

use jiff::{Span, civil::Date};

use crate::{StorageError, UsageStore};

/// Number of rolling calendar days represented by the home-page usage trend.
pub const USAGE_TREND_DAYS: usize = 7;

/// Aggregate usage data consumed by presentation-layer home pages.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsageSummary {
    pub total_duration_ms: u64,
    pub total_characters: u64,
    /// Daily duration ordered from oldest to newest.
    pub daily_duration_ms: [u64; USAGE_TREND_DAYS],
    /// Daily non-whitespace character count ordered from oldest to newest.
    pub daily_characters: [u64; USAGE_TREND_DAYS],
    /// Index of the first busiest day, or `None` when the trend is empty.
    pub highlighted_day: Option<usize>,
}

pub fn load_usage_summary(
    store: &dyn UsageStore,
    today: Date,
) -> Result<UsageSummary, StorageError> {
    let period_start = today
        .checked_sub(Span::new().days((USAGE_TREND_DAYS - 1) as i64))
        .map_err(|_| StorageError::Invalid("usage period starts outside the date range".into()))?;
    let snapshot = store.usage_snapshot(period_start, today)?;
    let mut daily_duration_ms = [0_u64; USAGE_TREND_DAYS];
    let mut daily_characters = [0_u64; USAGE_TREND_DAYS];
    for day in snapshot.days {
        let day_index = (day.date - period_start).get_days();
        if let Ok(day_index) = usize::try_from(day_index)
            && day_index < USAGE_TREND_DAYS
        {
            daily_duration_ms[day_index] = day.audio_duration_ms;
            daily_characters[day_index] = day.character_count;
        }
    }
    let maximum = daily_duration_ms.iter().copied().max().unwrap_or_default();
    let highlighted_day = daily_duration_ms
        .iter()
        .position(|duration| maximum > 0 && *duration == maximum);

    Ok(UsageSummary {
        total_duration_ms: snapshot.total_duration_ms,
        total_characters: snapshot.total_characters,
        daily_duration_ms,
        daily_characters,
        highlighted_day,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use jiff::civil::date;

    use crate::{DailyUsage, UsageSnapshot};

    use super::*;

    #[test]
    fn maps_persisted_buckets_into_the_rolling_window() {
        for (outside, first, yesterday, today) in [
            (
                date(2024, 2, 28),
                date(2024, 2, 29),
                date(2024, 3, 5),
                date(2024, 3, 6),
            ),
            (
                date(2025, 12, 28),
                date(2025, 12, 29),
                date(2026, 1, 3),
                date(2026, 1, 4),
            ),
        ] {
            let store = FakeUsageStore::new(Ok(UsageSnapshot {
                total_duration_ms: 150_000,
                total_characters: 14,
                days: vec![
                    day(outside, 50_000, 3),
                    day(first, 10_000, 1),
                    day(yesterday, 60_000, 3),
                    day(today, 30_000, 7),
                ],
            }));

            assert_eq!(
                Ok(UsageSummary {
                    total_duration_ms: 150_000,
                    total_characters: 14,
                    daily_duration_ms: [10_000, 0, 0, 0, 0, 60_000, 30_000],
                    daily_characters: [1, 0, 0, 0, 0, 3, 7],
                    highlighted_day: Some(5),
                }),
                load_usage_summary(&store, today)
            );
        }
    }

    #[test]
    fn empty_usage_has_zero_totals_and_no_highlight() {
        let store = FakeUsageStore::new(Ok(UsageSnapshot::default()));

        assert_eq!(
            Ok(UsageSummary::default()),
            load_usage_summary(&store, date(2026, 7, 15))
        );
    }

    #[test]
    fn loader_propagates_storage_errors() {
        let error = StorageError::Unavailable("usage offline".to_owned());
        let store = FakeUsageStore::new(Err(error.clone()));

        assert_eq!(Err(error), load_usage_summary(&store, date(2026, 7, 15)));
    }

    #[test]
    fn rejects_a_window_that_precedes_the_minimum_representable_date() {
        let store = FakeUsageStore::new(Ok(UsageSnapshot::default()));
        assert!(matches!(
            load_usage_summary(&store, Date::MIN),
            Err(StorageError::Invalid(_))
        ));
    }

    fn day(date: Date, duration: u64, characters: u64) -> DailyUsage {
        DailyUsage {
            date,
            audio_duration_ms: duration,
            character_count: characters,
        }
    }

    struct FakeUsageStore {
        snapshot: Mutex<Option<Result<UsageSnapshot, StorageError>>>,
    }

    impl FakeUsageStore {
        fn new(snapshot: Result<UsageSnapshot, StorageError>) -> Self {
            Self {
                snapshot: Mutex::new(Some(snapshot)),
            }
        }
    }

    impl UsageStore for FakeUsageStore {
        fn usage_snapshot(
            &self,
            _period_start: Date,
            _period_end: Date,
        ) -> Result<UsageSnapshot, StorageError> {
            self.snapshot
                .lock()
                .map_err(|_| StorageError::Unavailable("usage lock failed".to_owned()))?
                .take()
                .unwrap_or_else(|| Err(StorageError::Unavailable("usage exhausted".to_owned())))
        }
    }
}
