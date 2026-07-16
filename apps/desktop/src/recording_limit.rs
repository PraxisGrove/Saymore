use std::sync::atomic::{AtomicBool, Ordering};

pub const WARNING_AT_MS: u64 = 9 * 60 * 1_000;
pub const LIMIT_AT_MS: u64 = 10 * 60 * 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordingLimitEvent {
    None,
    Warn,
    Finish,
}

#[derive(Default)]
pub struct RecordingLimitTracker {
    warning_sent: AtomicBool,
    finish_sent: AtomicBool,
}

impl RecordingLimitTracker {
    pub fn observe(&self, elapsed_ms: u64) -> RecordingLimitEvent {
        if elapsed_ms >= LIMIT_AT_MS {
            self.warning_sent.store(true, Ordering::Release);
            return if self.finish_sent.swap(true, Ordering::AcqRel) {
                RecordingLimitEvent::None
            } else {
                RecordingLimitEvent::Finish
            };
        }
        if elapsed_ms >= WARNING_AT_MS && !self.warning_sent.swap(true, Ordering::AcqRel) {
            return RecordingLimitEvent::Warn;
        }
        RecordingLimitEvent::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warning_and_finish_are_each_emitted_once() {
        let tracker = RecordingLimitTracker::default();

        assert_eq!(
            RecordingLimitEvent::None,
            tracker.observe(WARNING_AT_MS - 1)
        );
        assert_eq!(RecordingLimitEvent::Warn, tracker.observe(WARNING_AT_MS));
        assert_eq!(
            RecordingLimitEvent::None,
            tracker.observe(WARNING_AT_MS + 1)
        );
        assert_eq!(RecordingLimitEvent::Finish, tracker.observe(LIMIT_AT_MS));
        assert_eq!(RecordingLimitEvent::None, tracker.observe(LIMIT_AT_MS + 1));
    }

    #[test]
    fn a_late_first_metric_finishes_without_showing_a_stale_warning() {
        let tracker = RecordingLimitTracker::default();

        assert_eq!(RecordingLimitEvent::Finish, tracker.observe(LIMIT_AT_MS));
        assert_eq!(RecordingLimitEvent::None, tracker.observe(LIMIT_AT_MS));
    }
}
