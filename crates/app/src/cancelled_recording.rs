use std::time::{Duration, Instant};

use crate::{DictationHandoff, DictationSessionId, PcmRecording};

pub struct CancelledRecordingStore {
    retained: Option<RetainedRecording>,
    retention: Duration,
    generation: u64,
}

struct RetainedRecording {
    id: DictationSessionId,
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

    pub fn retain(&mut self, id: DictationSessionId, recording: PcmRecording, now: Instant) -> u64 {
        self.generation = self.generation.wrapping_add(1);
        self.retained = Some(RetainedRecording {
            id,
            recording,
            cancelled_at: now,
            generation: self.generation,
        });
        self.generation
    }

    pub fn take(&mut self, now: Instant) -> Option<DictationHandoff> {
        self.remove_if_expired(now);
        self.retained
            .take()
            .map(|retained| DictationHandoff::Restored {
                id: retained.id,
                recording: retained.recording,
            })
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
        let id = DictationSessionId::generate();
        let mut store = CancelledRecordingStore::new(Duration::from_secs(10));
        store.retain(id, recording(), now);

        assert!(matches!(
            store.take(now + Duration::from_secs(9)),
            Some(DictationHandoff::Restored {
                id: restored_id,
                recording: restored_recording,
            }) if restored_id == id && restored_recording == recording()
        ));
    }

    #[test]
    fn destroys_cancelled_audio_after_the_undo_window() {
        let now = Instant::now();
        let mut store = CancelledRecordingStore::new(Duration::from_secs(10));
        let generation = store.retain(DictationSessionId::generate(), recording(), now);

        assert!(store.expire(generation, now + Duration::from_secs(10)));
        assert!(store.take(now + Duration::from_secs(10)).is_none());
    }

    #[test]
    fn old_expiration_does_not_remove_a_newer_cancellation() {
        let now = Instant::now();
        let id = DictationSessionId::generate();
        let mut store = CancelledRecordingStore::new(Duration::from_secs(10));
        let first = store.retain(id, recording(), now);
        store.retain(id, recording(), now + Duration::from_secs(2));

        assert!(!store.expire(first, now + Duration::from_secs(11)));
        assert!(matches!(
            store.take(now + Duration::from_secs(11)),
            Some(DictationHandoff::Restored {
                id: restored_id,
                recording: restored_recording,
            }) if restored_id == id && restored_recording == recording()
        ));
    }
}
