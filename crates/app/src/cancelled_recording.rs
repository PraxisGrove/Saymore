use std::time::{Duration, Instant};

use crate::PcmRecording;

pub struct CancelledRecordingStore {
    retained: Option<RetainedRecording>,
    retention: Duration,
    generation: u64,
}

struct RetainedRecording {
    recording: PcmRecording,
    cancelled_at: Instant,
    generation: u64,
}

impl CancelledRecordingStore {
    pub fn new(retention: Duration) -> Self {
        Self {
            retained: None,
            retention,
            generation: 0,
        }
    }

    pub fn retain(&mut self, recording: PcmRecording, now: Instant) -> u64 {
        self.generation = self.generation.wrapping_add(1);
        self.retained = Some(RetainedRecording {
            recording,
            cancelled_at: now,
            generation: self.generation,
        });
        self.generation
    }

    pub fn take(&mut self, now: Instant) -> Option<PcmRecording> {
        self.remove_if_expired(now);
        self.retained.take().map(|retained| retained.recording)
    }

    pub fn expire(&mut self, generation: u64, now: Instant) -> bool {
        let should_expire = self.retained.as_ref().is_some_and(|retained| {
            retained.generation == generation
                && now.duration_since(retained.cancelled_at) >= self.retention
        });
        if should_expire {
            self.retained = None;
        }
        should_expire
    }

    pub fn clear(&mut self) {
        self.retained = None;
    }

    fn remove_if_expired(&mut self, now: Instant) {
        let generation = self.retained.as_ref().map(|retained| retained.generation);
        if let Some(generation) = generation {
            self.expire(generation, now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recording() -> PcmRecording {
        PcmRecording {
            samples: vec![1, 2, 3],
            sample_rate: 16_000,
            channels: 1,
            duration_ms: 30,
        }
    }

    #[test]
    fn returns_cancelled_audio_during_the_undo_window() {
        let now = Instant::now();
        let mut store = CancelledRecordingStore::new(Duration::from_secs(10));
        store.retain(recording(), now);

        assert_eq!(Some(recording()), store.take(now + Duration::from_secs(9)));
    }

    #[test]
    fn destroys_cancelled_audio_after_the_undo_window() {
        let now = Instant::now();
        let mut store = CancelledRecordingStore::new(Duration::from_secs(10));
        let generation = store.retain(recording(), now);

        assert!(store.expire(generation, now + Duration::from_secs(10)));
        assert_eq!(None, store.take(now + Duration::from_secs(10)));
    }

    #[test]
    fn old_expiration_does_not_remove_a_newer_cancellation() {
        let now = Instant::now();
        let mut store = CancelledRecordingStore::new(Duration::from_secs(10));
        let first = store.retain(recording(), now);
        store.retain(recording(), now + Duration::from_secs(2));

        assert!(!store.expire(first, now + Duration::from_secs(11)));
        assert_eq!(Some(recording()), store.take(now + Duration::from_secs(11)));
    }
}
