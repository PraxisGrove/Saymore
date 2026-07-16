use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DictationSessionState {
    Idle = 0,
    Starting = 1,
    Recording = 2,
    Finishing = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DictationToggleAction {
    Start,
    Finish,
    IgnorePaused,
    IgnoreStarting,
    IgnoreFinishing,
}

/// Owns the thread-safe state transitions for one dictation workflow.
///
/// Desktop adapters execute the returned actions and report asynchronous start,
/// finish, and cancellation outcomes back to this state machine.
pub struct DictationSession {
    state: AtomicU8,
}

impl Default for DictationSession {
    fn default() -> Self {
        Self {
            state: AtomicU8::new(DictationSessionState::Idle as u8),
        }
    }
}

impl DictationSession {
    pub fn request_toggle(&self, paused: bool) -> DictationToggleAction {
        if paused {
            return DictationToggleAction::IgnorePaused;
        }
        loop {
            match self.state() {
                DictationSessionState::Idle => {
                    if self
                        .transition(DictationSessionState::Idle, DictationSessionState::Starting)
                        .is_ok()
                    {
                        return DictationToggleAction::Start;
                    }
                }
                DictationSessionState::Starting => {
                    return DictationToggleAction::IgnoreStarting;
                }
                DictationSessionState::Recording => {
                    if self
                        .transition(
                            DictationSessionState::Recording,
                            DictationSessionState::Finishing,
                        )
                        .is_ok()
                    {
                        return DictationToggleAction::Finish;
                    }
                }
                DictationSessionState::Finishing => {
                    return DictationToggleAction::IgnoreFinishing;
                }
            }
        }
    }

    pub fn recording_started(&self) -> bool {
        self.transition(
            DictationSessionState::Starting,
            DictationSessionState::Recording,
        )
        .is_ok()
    }

    pub fn startup_failed(&self) {
        let _ = self.transition(DictationSessionState::Starting, DictationSessionState::Idle);
    }

    pub fn request_finish(&self) -> bool {
        self.transition(
            DictationSessionState::Recording,
            DictationSessionState::Finishing,
        )
        .is_ok()
    }

    pub fn begin_retained_processing(&self) -> bool {
        self.transition(
            DictationSessionState::Idle,
            DictationSessionState::Finishing,
        )
        .is_ok()
    }

    pub fn request_cancel(&self) -> bool {
        loop {
            let current = self.state();
            if !matches!(
                current,
                DictationSessionState::Starting | DictationSessionState::Recording
            ) {
                return false;
            }
            if self
                .transition(current, DictationSessionState::Idle)
                .is_ok()
            {
                return true;
            }
        }
    }

    pub fn complete(&self) {
        self.state
            .store(DictationSessionState::Idle as u8, Ordering::Release);
    }

    pub fn is_recording(&self) -> bool {
        self.state() == DictationSessionState::Recording
    }

    pub fn state(&self) -> DictationSessionState {
        match self.state.load(Ordering::Acquire) {
            1 => DictationSessionState::Starting,
            2 => DictationSessionState::Recording,
            3 => DictationSessionState::Finishing,
            _ => DictationSessionState::Idle,
        }
    }

    fn transition(
        &self,
        current: DictationSessionState,
        next: DictationSessionState,
    ) -> Result<u8, u8> {
        self.state.compare_exchange(
            current as u8,
            next as u8,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_drives_one_complete_dictation_lifecycle() {
        let session = DictationSession::default();

        assert_eq!(DictationToggleAction::Start, session.request_toggle(false));
        assert_eq!(DictationSessionState::Starting, session.state());
        assert!(session.recording_started());
        assert_eq!(DictationToggleAction::Finish, session.request_toggle(false));
        assert_eq!(DictationSessionState::Finishing, session.state());
        session.complete();
        assert_eq!(DictationSessionState::Idle, session.state());
    }

    #[test]
    fn duplicate_or_paused_requests_do_not_create_overlapping_work() {
        let session = DictationSession::default();

        assert_eq!(
            DictationToggleAction::IgnorePaused,
            session.request_toggle(true)
        );
        assert_eq!(DictationToggleAction::Start, session.request_toggle(false));
        assert_eq!(
            DictationToggleAction::IgnoreStarting,
            session.request_toggle(false)
        );
        assert!(session.recording_started());
        assert!(session.request_finish());
        assert_eq!(
            DictationToggleAction::IgnoreFinishing,
            session.request_toggle(false)
        );
    }

    #[test]
    fn failures_cancellation_and_retained_processing_return_to_idle() {
        let session = DictationSession::default();

        assert_eq!(DictationToggleAction::Start, session.request_toggle(false));
        session.startup_failed();
        assert_eq!(DictationSessionState::Idle, session.state());

        assert_eq!(DictationToggleAction::Start, session.request_toggle(false));
        assert!(session.recording_started());
        assert!(session.request_cancel());
        assert!(session.begin_retained_processing());
        session.complete();
        assert_eq!(DictationSessionState::Idle, session.state());
    }
}
