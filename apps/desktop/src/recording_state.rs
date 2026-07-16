use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use template_app::{CancelledRecordingStore, DictationSession};

pub(crate) fn initialize(
    cancel_undo_window: Duration,
) -> (Arc<DictationSession>, Arc<Mutex<CancelledRecordingStore>>) {
    (
        Arc::new(DictationSession::default()),
        Arc::new(Mutex::new(CancelledRecordingStore::new(cancel_undo_window))),
    )
}
