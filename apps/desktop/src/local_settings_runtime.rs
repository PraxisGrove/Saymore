use std::{
    io,
    sync::{
        Arc, Mutex,
        mpsc::{self, SyncSender, TrySendError},
    },
    thread::{self, JoinHandle},
};

use template_app::{
    LocalSettings, LocalSettingsChange, LocalSettingsMutationError, LocalSettingsMutator,
};

const SETTINGS_QUEUE_CAPACITY: usize = 32;

type MutationResult = Result<LocalSettings, LocalSettingsMutationError>;
type Completion = Box<dyn FnOnce(MutationResult) + Send>;
type Dispatcher = Arc<dyn Fn(Completion, MutationResult) -> Result<(), String> + Send + Sync>;

struct WorkItem {
    change: LocalSettingsChange,
    completion: Completion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalSettingsSubmissionError {
    QueueFull,
    ShuttingDown,
}

impl std::fmt::Display for LocalSettingsSubmissionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::QueueFull => formatter.write_str("the local settings queue is full"),
            Self::ShuttingDown => formatter.write_str("local settings mutation is shutting down"),
        }
    }
}

impl std::error::Error for LocalSettingsSubmissionError {}

#[derive(Clone)]
pub struct LocalSettingsHandle {
    sender: Arc<Mutex<Option<SyncSender<WorkItem>>>>,
}

impl LocalSettingsHandle {
    pub fn submit(
        &self,
        change: LocalSettingsChange,
        completion: impl FnOnce(MutationResult) + Send + 'static,
    ) -> Result<(), LocalSettingsSubmissionError> {
        let sender = self
            .sender
            .lock()
            .map_err(|_| LocalSettingsSubmissionError::ShuttingDown)?;
        let sender = sender
            .as_ref()
            .ok_or(LocalSettingsSubmissionError::ShuttingDown)?;
        let item = WorkItem {
            change,
            completion: Box::new(completion),
        };
        sender.try_send(item).map_err(|error| match error {
            TrySendError::Full(_) => LocalSettingsSubmissionError::QueueFull,
            TrySendError::Disconnected(_) => LocalSettingsSubmissionError::ShuttingDown,
        })
    }
}

pub struct LocalSettingsRuntime {
    sender: Arc<Mutex<Option<SyncSender<WorkItem>>>>,
    worker: Option<JoinHandle<()>>,
}

impl LocalSettingsRuntime {
    pub fn new(mutator: Arc<LocalSettingsMutator>) -> io::Result<Self> {
        Self::with_dispatcher(
            mutator,
            SETTINGS_QUEUE_CAPACITY,
            Arc::new(|completion, result| {
                slint::invoke_from_event_loop(move || completion(result))
                    .map_err(|error| error.to_string())
            }),
        )
    }

    fn with_dispatcher(
        mutator: Arc<LocalSettingsMutator>,
        capacity: usize,
        dispatcher: Dispatcher,
    ) -> io::Result<Self> {
        let (sender, receiver) = mpsc::sync_channel::<WorkItem>(capacity);
        let sender = Arc::new(Mutex::new(Some(sender)));
        let worker = thread::Builder::new()
            .name("saymore-local-settings".to_owned())
            .spawn(move || {
                while let Ok(item) = receiver.recv() {
                    let result = mutator.apply(item.change);
                    if let Err(error) = dispatcher(item.completion, result) {
                        tracing::warn!(
                            event = "settings.completion_dispatch_failed",
                            reason = %error
                        );
                    }
                }
            })?;
        Ok(Self {
            sender,
            worker: Some(worker),
        })
    }

    pub fn handle(&self) -> LocalSettingsHandle {
        LocalSettingsHandle {
            sender: Arc::clone(&self.sender),
        }
    }
}

impl Drop for LocalSettingsRuntime {
    fn drop(&mut self) {
        match self.sender.lock() {
            Ok(mut sender) => {
                sender.take();
            }
            Err(poisoned) => {
                tracing::warn!(event = "settings.runtime_state_poisoned");
                poisoned.into_inner().take();
            }
        }
        if let Some(worker) = self.worker.take()
            && worker.join().is_err()
        {
            tracing::error!(event = "settings.worker_panicked");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    };

    use template_app::{LocalSettingsStore, StorageError};

    use super::*;

    struct FakeStore {
        settings: Mutex<LocalSettings>,
        fail_next_save: AtomicBool,
    }

    impl FakeStore {
        fn new() -> Self {
            Self {
                settings: Mutex::new(LocalSettings::default()),
                fail_next_save: AtomicBool::new(false),
            }
        }
    }

    impl LocalSettingsStore for FakeStore {
        fn load_settings(&self) -> Result<LocalSettings, StorageError> {
            self.settings
                .lock()
                .map(|settings| settings.clone())
                .map_err(|_| StorageError::Unavailable("fake lock poisoned".to_owned()))
        }

        fn save_settings(&self, settings: LocalSettings) -> Result<(), StorageError> {
            if self.fail_next_save.swap(false, Ordering::AcqRel) {
                return Err(StorageError::Unavailable("injected failure".to_owned()));
            }
            self.settings
                .lock()
                .map(|mut stored| *stored = settings)
                .map_err(|_| StorageError::Unavailable("fake lock poisoned".to_owned()))
        }
    }

    fn direct_dispatcher() -> Dispatcher {
        Arc::new(|completion, result| {
            completion(result);
            Ok(())
        })
    }

    fn runtime(store: Arc<FakeStore>) -> LocalSettingsRuntime {
        LocalSettingsRuntime::with_dispatcher(
            Arc::new(LocalSettingsMutator::new(store)),
            4,
            direct_dispatcher(),
        )
        .unwrap_or_else(|error| panic!("runtime should start: {error}"))
    }

    #[test]
    fn accepted_changes_complete_in_fifo_order() {
        let runtime = runtime(Arc::new(FakeStore::new()));
        let handle = runtime.handle();
        let (completed, results) = mpsc::channel();
        for (label, change) in [
            ("feedback", LocalSettingsChange::SetFeedbackSounds(false)),
            ("clipboard", LocalSettingsChange::SetCopyToClipboard(true)),
        ] {
            let completed = completed.clone();
            assert!(
                handle
                    .submit(change, move |result| {
                        let _ = completed.send((label, result));
                    })
                    .is_ok()
            );
        }
        drop(completed);
        drop(runtime);

        let labels = results
            .into_iter()
            .map(|(label, result)| {
                assert!(result.is_ok());
                label
            })
            .collect::<Vec<_>>();
        assert_eq!(vec!["feedback", "clipboard"], labels);
    }

    #[test]
    fn one_failed_change_does_not_stop_later_work() {
        let store = Arc::new(FakeStore::new());
        store.fail_next_save.store(true, Ordering::Release);
        let runtime = runtime(store);
        let handle = runtime.handle();
        let (completed, results) = mpsc::channel();
        for change in [
            LocalSettingsChange::SetFeedbackSounds(false),
            LocalSettingsChange::SetCopyToClipboard(true),
        ] {
            let completed = completed.clone();
            assert!(
                handle
                    .submit(change, move |result| {
                        let _ = completed.send(result);
                    })
                    .is_ok()
            );
        }
        drop(completed);
        drop(runtime);

        let results = results.into_iter().collect::<Vec<_>>();
        assert!(results[0].is_err());
        assert!(results[1].is_ok());
    }

    #[test]
    fn shutdown_drains_accepted_work_and_rejects_later_submissions() {
        let runtime = runtime(Arc::new(FakeStore::new()));
        let handle = runtime.handle();
        let (completed, result) = mpsc::channel();
        assert!(
            handle
                .submit(
                    LocalSettingsChange::SetDictationPaused(true),
                    move |value| {
                        let _ = completed.send(value);
                    }
                )
                .is_ok()
        );

        drop(runtime);

        assert!(result.recv().is_ok_and(|value| value.is_ok()));
        assert_eq!(
            Err(LocalSettingsSubmissionError::ShuttingDown),
            handle.submit(LocalSettingsChange::SetDictationPaused(false), |_| {})
        );
    }

    #[test]
    fn submit_racing_shutdown_is_either_completed_or_rejected() {
        let runtime = runtime(Arc::new(FakeStore::new()));
        let handle = runtime.handle();
        let Ok(state_guard) = handle.sender.lock() else {
            panic!("runtime sender state should be available");
        };
        let racing_handle = handle.clone();
        let (completed, completion) = mpsc::channel();
        let submitter = std::thread::spawn(move || {
            racing_handle.submit(LocalSettingsChange::SetDictationPaused(true), move |_| {
                let _ = completed.send(());
            })
        });
        let shutdown = std::thread::spawn(move || drop(runtime));

        drop(state_guard);

        let Ok(submission) = submitter.join() else {
            panic!("submission thread should not panic");
        };
        assert!(shutdown.join().is_ok());
        match submission {
            Ok(()) => assert_eq!(Ok(()), completion.recv()),
            Err(error) => {
                assert_eq!(LocalSettingsSubmissionError::ShuttingDown, error);
                assert_eq!(Err(mpsc::TryRecvError::Disconnected), completion.try_recv());
            }
        }
    }

    #[test]
    fn dispatcher_failure_does_not_invoke_the_completion() {
        let runtime = LocalSettingsRuntime::with_dispatcher(
            Arc::new(LocalSettingsMutator::new(Arc::new(FakeStore::new()))),
            1,
            Arc::new(|_, _| Err("event loop closed".to_owned())),
        )
        .unwrap_or_else(|error| panic!("runtime should start: {error}"));
        let handle = runtime.handle();
        let (completed, result) = mpsc::channel::<()>();
        assert!(
            handle
                .submit(LocalSettingsChange::SetDictationPaused(true), move |_| {
                    let _ = completed.send(());
                })
                .is_ok()
        );

        drop(runtime);

        assert_eq!(Err(mpsc::TryRecvError::Disconnected), result.try_recv());
    }

    struct BlockingStore {
        entered: mpsc::Sender<()>,
        release: Mutex<mpsc::Receiver<()>>,
    }

    impl LocalSettingsStore for BlockingStore {
        fn load_settings(&self) -> Result<LocalSettings, StorageError> {
            self.entered
                .send(())
                .map_err(|error| StorageError::Unavailable(error.to_string()))?;
            self.release
                .lock()
                .map_err(|_| StorageError::Unavailable("release lock poisoned".to_owned()))?
                .recv()
                .map_err(|error| StorageError::Unavailable(error.to_string()))?;
            Ok(LocalSettings::default())
        }

        fn save_settings(&self, _settings: LocalSettings) -> Result<(), StorageError> {
            Ok(())
        }
    }

    #[test]
    fn full_queue_is_reported_without_blocking() {
        let (entered, worker_entered) = mpsc::channel();
        let (release, worker_release) = mpsc::channel();
        let runtime = LocalSettingsRuntime::with_dispatcher(
            Arc::new(LocalSettingsMutator::new(Arc::new(BlockingStore {
                entered,
                release: Mutex::new(worker_release),
            }))),
            1,
            direct_dispatcher(),
        )
        .unwrap_or_else(|error| panic!("runtime should start: {error}"));
        let handle = runtime.handle();
        assert!(
            handle
                .submit(LocalSettingsChange::SetDictationPaused(true), |_| {})
                .is_ok()
        );
        assert!(worker_entered.recv().is_ok());
        assert!(
            handle
                .submit(LocalSettingsChange::SetDictationPaused(false), |_| {})
                .is_ok()
        );

        assert_eq!(
            Err(LocalSettingsSubmissionError::QueueFull),
            handle.submit(LocalSettingsChange::SetDictationPaused(true), |_| {})
        );

        assert!(release.send(()).is_ok());
        assert!(release.send(()).is_ok());
        drop(runtime);
    }
}
