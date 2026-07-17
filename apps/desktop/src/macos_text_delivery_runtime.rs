use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, ThreadId},
    time::Duration,
};

use template_app::{
    AccessibilityAuthorization, CorrectionObservingTextDeliverer, DeliveryTargetPrivacy,
    TextDeliverer, TextDeliveryError, TextDeliveryOutcome, TextEditObserver,
};

const DELIVERY_TIMEOUT: Duration = Duration::from_secs(5);

type DispatchTask = Box<dyn FnOnce() + Send + 'static>;
type EventLoopDispatcher = Arc<dyn Fn(DispatchTask) -> Result<(), String> + Send + Sync>;

pub(crate) struct MacOsMainThreadTextDeliverer {
    platform: Arc<dyn CorrectionObservingTextDeliverer>,
    main_thread: ThreadId,
    dispatcher: EventLoopDispatcher,
    timeout: Duration,
}

impl MacOsMainThreadTextDeliverer {
    pub(crate) fn new(platform: Arc<dyn CorrectionObservingTextDeliverer>) -> Self {
        Self {
            platform,
            main_thread: thread::current().id(),
            dispatcher: Arc::new(|task| {
                slint::invoke_from_event_loop(task).map_err(|error| error.to_string())
            }),
            timeout: DELIVERY_TIMEOUT,
        }
    }

    fn run_on_main<T>(
        &self,
        operation: impl FnOnce(Arc<dyn CorrectionObservingTextDeliverer>) -> T + Send + 'static,
    ) -> Result<T, TextDeliveryError>
    where
        T: Send + 'static,
    {
        if thread::current().id() == self.main_thread {
            return Ok(operation(Arc::clone(&self.platform)));
        }

        let (result_tx, result_rx) = mpsc::sync_channel(1);
        let platform = Arc::clone(&self.platform);
        let cancelled = Arc::new(AtomicBool::new(false));
        let task_cancelled = Arc::clone(&cancelled);
        (self.dispatcher)(Box::new(move || {
            if task_cancelled.load(Ordering::Acquire) {
                return;
            }
            let _ = result_tx.send(operation(platform));
        }))
        .map_err(|error| {
            TextDeliveryError::System(format!(
                "schedule macOS text delivery on the main thread failed: {error}"
            ))
        })?;

        result_rx.recv_timeout(self.timeout).map_err(|error| {
            cancelled.store(true, Ordering::Release);
            TextDeliveryError::System(format!(
                "wait for macOS main-thread text delivery failed: {error}"
            ))
        })
    }
}

impl TextDeliverer for MacOsMainThreadTextDeliverer {
    fn authorization(&self) -> AccessibilityAuthorization {
        match self.run_on_main(|platform| platform.authorization()) {
            Ok(authorization) => authorization,
            Err(error) => {
                tracing::warn!(event = "macos.authorization_dispatch_failed", reason = %error);
                AccessibilityAuthorization::Denied
            }
        }
    }

    fn request_authorization(&self) -> AccessibilityAuthorization {
        match self.run_on_main(|platform| platform.request_authorization()) {
            Ok(authorization) => authorization,
            Err(error) => {
                tracing::warn!(event = "macos.authorization_request_dispatch_failed", reason = %error);
                AccessibilityAuthorization::Denied
            }
        }
    }

    fn target_privacy(&self) -> DeliveryTargetPrivacy {
        match self.run_on_main(|platform| platform.target_privacy()) {
            Ok(privacy) => privacy,
            Err(error) => {
                tracing::warn!(event = "macos.target_privacy_dispatch_failed", reason = %error);
                DeliveryTargetPrivacy::Sensitive
            }
        }
    }

    fn deliver(&self, text: &str) -> Result<TextDeliveryOutcome, TextDeliveryError> {
        let text = text.to_owned();
        self.run_on_main(move |platform| platform.deliver(&text))?
    }
}

impl CorrectionObservingTextDeliverer for MacOsMainThreadTextDeliverer {
    fn deliver_and_observe(
        &self,
        text: &str,
        observer: TextEditObserver,
    ) -> Result<TextDeliveryOutcome, TextDeliveryError> {
        let text = text.to_owned();
        self.run_on_main(move |platform| platform.deliver_and_observe(&text, observer))?
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[derive(Default)]
    struct FakeDeliverer {
        delivery_thread: Mutex<Option<ThreadId>>,
    }

    impl TextDeliverer for FakeDeliverer {
        fn authorization(&self) -> AccessibilityAuthorization {
            AccessibilityAuthorization::Granted
        }

        fn request_authorization(&self) -> AccessibilityAuthorization {
            AccessibilityAuthorization::Granted
        }

        fn target_privacy(&self) -> DeliveryTargetPrivacy {
            DeliveryTargetPrivacy::Standard
        }

        fn deliver(&self, _text: &str) -> Result<TextDeliveryOutcome, TextDeliveryError> {
            self.delivery_thread
                .lock()
                .map_err(|_| TextDeliveryError::System("fake lock poisoned".to_owned()))?
                .replace(thread::current().id());
            Ok(TextDeliveryOutcome::AccessibilityVerified)
        }
    }

    impl CorrectionObservingTextDeliverer for FakeDeliverer {
        fn deliver_and_observe(
            &self,
            text: &str,
            _observer: TextEditObserver,
        ) -> Result<TextDeliveryOutcome, TextDeliveryError> {
            self.deliver(text)
        }
    }

    fn adapter_with_dispatcher(
        platform: Arc<dyn CorrectionObservingTextDeliverer>,
        dispatcher: EventLoopDispatcher,
    ) -> MacOsMainThreadTextDeliverer {
        MacOsMainThreadTextDeliverer {
            platform,
            main_thread: thread::current().id(),
            dispatcher,
            timeout: Duration::from_secs(1),
        }
    }

    #[test]
    fn background_delivery_runs_on_the_owner_thread() {
        let fake = Arc::new(FakeDeliverer::default());
        let platform: Arc<dyn CorrectionObservingTextDeliverer> = fake.clone();
        let owner_thread = thread::current().id();
        let (task_tx, task_rx) = mpsc::sync_channel::<DispatchTask>(1);
        let adapter = Arc::new(adapter_with_dispatcher(
            platform,
            Arc::new(move |task| task_tx.send(task).map_err(|error| error.to_string())),
        ));

        let worker = thread::spawn(move || adapter.deliver("hello"));
        let task = task_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap_or_else(|error| panic!("main-thread task should arrive: {error}"));
        task();
        let result = worker
            .join()
            .unwrap_or_else(|_| panic!("delivery worker should finish"));
        let delivery_thread = fake
            .delivery_thread
            .lock()
            .map(|thread| *thread)
            .unwrap_or_else(|_| panic!("fake delivery thread should be readable"));

        assert_eq!(Ok(TextDeliveryOutcome::AccessibilityVerified), result);
        assert_eq!(Some(owner_thread), delivery_thread);
    }

    #[test]
    fn owner_thread_delivery_does_not_dispatch_to_itself() {
        let fake = Arc::new(FakeDeliverer::default());
        let platform: Arc<dyn CorrectionObservingTextDeliverer> = fake;
        let adapter = adapter_with_dispatcher(
            platform,
            Arc::new(|_| Err("dispatcher must not run".to_owned())),
        );

        assert_eq!(
            Ok(TextDeliveryOutcome::AccessibilityVerified),
            adapter.deliver("hello")
        );
    }

    #[test]
    fn dispatch_failure_is_reported_as_a_delivery_error() {
        let platform: Arc<dyn CorrectionObservingTextDeliverer> =
            Arc::new(FakeDeliverer::default());
        let mut adapter =
            adapter_with_dispatcher(platform, Arc::new(|_| Err("event loop closed".to_owned())));
        adapter.main_thread = thread::spawn(|| thread::current().id())
            .join()
            .unwrap_or_else(|_| panic!("thread id worker should finish"));

        assert!(matches!(
            adapter.deliver("hello"),
            Err(TextDeliveryError::System(message))
                if message.contains("event loop closed")
        ));
    }

    #[test]
    fn timed_out_task_does_not_deliver_when_the_event_loop_recovers() {
        let fake = Arc::new(FakeDeliverer::default());
        let platform: Arc<dyn CorrectionObservingTextDeliverer> = fake.clone();
        let (task_tx, task_rx) = mpsc::sync_channel::<DispatchTask>(1);
        let mut adapter = adapter_with_dispatcher(
            platform,
            Arc::new(move |task| task_tx.send(task).map_err(|error| error.to_string())),
        );
        adapter.main_thread = thread::spawn(|| thread::current().id())
            .join()
            .unwrap_or_else(|_| panic!("thread id worker should finish"));
        adapter.timeout = Duration::from_millis(1);

        assert!(adapter.deliver("hello").is_err());
        let task = task_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap_or_else(|error| panic!("delayed task should remain queued: {error}"));
        task();

        assert!(
            fake.delivery_thread
                .lock()
                .is_ok_and(|thread| thread.is_none())
        );
    }
}
