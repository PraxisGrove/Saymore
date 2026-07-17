use std::time::{SystemTime, UNIX_EPOCH};

use template_app::DictationCompletionClock;

/// Supplies saturated UTC timestamps to the dictation completion module.
pub struct SystemClock;

impl DictationCompletionClock for SystemClock {
    fn now_ms(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
            .unwrap_or_default()
    }
}
