use std::{
    sync::{
        Mutex,
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
};

use template_app::{
    AccessibilityAuthorization, DeliveryTargetPrivacy, TextDeliveryError, TextDeliveryOutcome,
    TextRevisionEndReason, TextRevisionObserver,
};

use super::{NativeDelivery, deliver_once, worker_unavailable};
use crate::windows_text_delivery::observation::{
    ActiveCorrectionObservation, CorrectionObservationTarget, POLL_INTERVAL,
};

const COMMAND_QUEUE_CAPACITY: usize = 2;

pub(super) enum DeliveryCommand {
    Authorization {
        response: mpsc::Sender<AccessibilityAuthorization>,
    },
    TargetPrivacy {
        response: mpsc::Sender<DeliveryTargetPrivacy>,
    },
    Deliver {
        text: String,
        observer: Option<TextRevisionObserver>,
        response: mpsc::Sender<Result<TextDeliveryOutcome, TextDeliveryError>>,
    },
    FinishObservation {
        reason: TextRevisionEndReason,
        response: mpsc::Sender<()>,
    },
}

pub(super) struct DeliveryWorker {
    sender: Mutex<Option<SyncSender<DeliveryCommand>>>,
    thread: Option<JoinHandle<()>>,
}

impl DeliveryWorker {
    pub(super) fn spawn() -> Result<Self, TextDeliveryError> {
        let (sender, receiver) = mpsc::sync_channel(COMMAND_QUEUE_CAPACITY);
        let (initialization_sender, initialization_receiver) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("saymore-windows-text-delivery".to_owned())
            .spawn(move || run(receiver, initialization_sender))
            .map_err(|error| {
                TextDeliveryError::System(format!("start Windows delivery worker failed: {error}"))
            })?;
        match initialization_receiver.recv() {
            Ok(Ok(())) => Ok(Self {
                sender: Mutex::new(Some(sender)),
                thread: Some(worker),
            }),
            Ok(Err(error)) => {
                drop(sender);
                let _ = worker.join();
                Err(error)
            }
            Err(_) => {
                drop(sender);
                let _ = worker.join();
                Err(worker_unavailable())
            }
        }
    }

    pub(super) fn request<T>(
        &self,
        command: impl FnOnce(mpsc::Sender<T>) -> DeliveryCommand,
    ) -> Result<T, TextDeliveryError> {
        let (response_sender, response_receiver) = mpsc::channel();
        let sender = self.sender.lock().map_err(|_| worker_unavailable())?;
        sender
            .as_ref()
            .ok_or_else(worker_unavailable)?
            .send(command(response_sender))
            .map_err(|_| worker_unavailable())?;
        drop(sender);
        response_receiver.recv().map_err(|_| worker_unavailable())
    }
}

impl Drop for DeliveryWorker {
    fn drop(&mut self) {
        match self.sender.lock() {
            Ok(mut sender) => {
                sender.take();
            }
            Err(poisoned) => {
                poisoned.into_inner().take();
            }
        }
        if let Some(worker) = self.thread.take() {
            let _ = worker.join();
        }
    }
}

fn run(
    receiver: Receiver<DeliveryCommand>,
    initialized: mpsc::Sender<Result<(), TextDeliveryError>>,
) {
    let runtime = match NativeDelivery::initialize() {
        Ok(runtime) => {
            let _ = initialized.send(Ok(()));
            runtime
        }
        Err(error) => {
            let _ = initialized.send(Err(error));
            return;
        }
    };
    let mut active_observations: Vec<ActiveCorrectionObservation> = Vec::new();
    loop {
        match receiver.recv_timeout(POLL_INTERVAL) {
            Ok(DeliveryCommand::Authorization { response }) => {
                let _ = response.send(AccessibilityAuthorization::Granted);
            }
            Ok(DeliveryCommand::TargetPrivacy { response }) => {
                let privacy = runtime
                    .focused_target()
                    .map(|target| target.privacy())
                    .unwrap_or(DeliveryTargetPrivacy::Sensitive);
                let _ = response.send(privacy);
            }
            Ok(DeliveryCommand::Deliver {
                text,
                observer,
                response,
            }) => {
                for observation in &mut active_observations {
                    observation.finish(TextRevisionEndReason::NextDictation);
                }
                active_observations.clear();
                match deliver_once(&runtime, &text) {
                    Ok(attempt) => {
                        if let Some(observation) = observer.and_then(|observer| {
                            CorrectionObservationTarget::capture(&attempt.target, &text)
                                .map(|target| ActiveCorrectionObservation::new(target, observer))
                        }) {
                            active_observations.push(observation);
                        }
                        let _ = response.send(Ok(attempt.outcome));
                    }
                    Err(error) => {
                        let _ = response.send(Err(error));
                    }
                }
            }
            Ok(DeliveryCommand::FinishObservation { reason, response }) => {
                for observation in &mut active_observations {
                    observation.finish(reason);
                }
                active_observations.clear();
                let _ = response.send(());
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                for observation in &mut active_observations {
                    observation.finish(TextRevisionEndReason::Cancelled);
                }
                break;
            }
        }
        active_observations.retain_mut(|observation| !observation.poll());
    }
}
