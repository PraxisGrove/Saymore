use std::{
    sync::{Arc, Mutex, atomic::AtomicBool},
    time::Duration,
};

use template_app::CancelledRecordingStore;

pub(crate) fn initialize(
    cancel_undo_window: Duration,
) -> (
    Arc<AtomicBool>,
    Arc<AtomicBool>,
    Arc<Mutex<CancelledRecordingStore>>,
) {
    (
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
        Arc::new(Mutex::new(CancelledRecordingStore::new(cancel_undo_window))),
    )
}
