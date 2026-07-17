use std::sync::{Mutex, MutexGuard};

use crate::DictationSessionId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DictationSessionState {
    Idle,
    Starting,
    Recording,
    Finishing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DictationToggleAction {
    Start,
    Finish(DictationSessionId),
    IgnorePaused,
    IgnoreStarting,
    IgnoreFinishing,
}

/// Owns the thread-safe state transitions for one dictation workflow.
///
/// Desktop adapters execute the returned actions and report asynchronous start,
/// finish, and cancellation outcomes back to this state machine.
pub struct DictationSession {
    data: Mutex<DictationSessionData>,
}

struct DictationSessionData {
    state: DictationSessionState,
    id: Option<DictationSessionId>,
}

impl Default for DictationSession {
    fn default() -> Self {
        Self {
            data: Mutex::new(DictationSessionData {
                state: DictationSessionState::Idle,
                id: None,
            }),
        }
    }
}

impl DictationSession {
    pub fn request_toggle(&self, paused: bool) -> DictationToggleAction {
        if paused {
            return DictationToggleAction::IgnorePaused;
        }
        let mut data = self.data();
        match data.state {
            DictationSessionState::Idle => {
                data.state = DictationSessionState::Starting;
                data.id = Some(DictationSessionId::generate());
                DictationToggleAction::Start
            }
            DictationSessionState::Starting => DictationToggleAction::IgnoreStarting,
            DictationSessionState::Recording => match data.id {
                Some(id) => {
                    data.state = DictationSessionState::Finishing;
                    DictationToggleAction::Finish(id)
                }
                None => DictationToggleAction::IgnoreFinishing,
            },
            DictationSessionState::Finishing => DictationToggleAction::IgnoreFinishing,
        }
    }

    pub fn recording_started(&self) -> bool {
        let mut data = self.data();
        if data.state != DictationSessionState::Starting {
            return false;
        }
        data.state = DictationSessionState::Recording;
        true
    }

    pub fn startup_failed(&self) {
        let mut data = self.data();
        if data.state == DictationSessionState::Starting {
            data.state = DictationSessionState::Idle;
            data.id = None;
        }
    }

    pub fn request_finish(&self) -> Option<DictationSessionId> {
        let mut data = self.data();
        if data.state != DictationSessionState::Recording {
            return None;
        }
        let id = data.id?;
        data.state = DictationSessionState::Finishing;
        Some(id)
    }

    pub fn begin_retained_processing(&self, id: DictationSessionId) -> bool {
        let mut data = self.data();
        if data.state != DictationSessionState::Idle {
            return false;
        }
        data.state = DictationSessionState::Finishing;
        data.id = Some(id);
        true
    }

    pub fn request_cancel(&self) -> bool {
        let mut data = self.data();
        if !matches!(
            data.state,
            DictationSessionState::Starting | DictationSessionState::Recording
        ) {
            return false;
        }
        data.state = DictationSessionState::Idle;
        true
    }

    pub fn complete(&self) {
        let mut data = self.data();
        data.state = DictationSessionState::Idle;
        data.id = None;
    }

    pub fn is_recording(&self) -> bool {
        self.state() == DictationSessionState::Recording
    }

    pub fn state(&self) -> DictationSessionState {
        self.data().state
    }

    pub fn current_id(&self) -> Option<DictationSessionId> {
        self.data().id
    }

    fn data(&self) -> MutexGuard<'_, DictationSessionData> {
        match self.data.lock() {
            Ok(data) => data,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_drives_one_complete_dictation_lifecycle() {
        let session = DictationSession::default();

        assert_eq!(DictationToggleAction::Start, session.request_toggle(false));
        let id = session.current_id();
        assert_eq!(DictationSessionState::Starting, session.state());
        assert!(session.recording_started());
        assert!(matches!(
            session.request_toggle(false),
            DictationToggleAction::Finish(finished_id) if Some(finished_id) == id
        ));
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
        assert!(session.request_finish().is_some());
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
        let id = session.current_id();
        assert!(id.is_some());
        assert!(session.request_cancel());
        if let Some(id) = id {
            assert!(session.begin_retained_processing(id));
        }
        session.complete();
        assert_eq!(DictationSessionState::Idle, session.state());
    }
}
