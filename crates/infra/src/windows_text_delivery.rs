mod clipboard;
mod observation;

use std::{
    fmt, mem,
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use template_app::{
    AccessibilityAuthorization, CorrectionObservingTextDeliverer, DeliveryTargetPrivacy,
    TextDeliverer, TextDeliveryError, TextDeliveryOutcome, TextEditObserver,
};
use windows::{
    Win32::{
        System::{
            Com::{CLSCTX_INPROC_SERVER, CoCreateInstance},
            Ole::{OleInitialize, OleUninitialize},
        },
        UI::{
            Accessibility::{
                CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTextEditPattern,
                IUIAutomationTextPattern, IUIAutomationValuePattern, UIA_TextEditPatternId,
                UIA_TextPatternId, UIA_ValuePatternId,
            },
            Input::KeyboardAndMouse::{
                INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
                VIRTUAL_KEY, VK_CONTROL, VK_V,
            },
        },
    },
    core::{Error as WindowsError, HRESULT, Interface},
};

use self::clipboard::{ClipboardSnapshot, TemporaryClipboard};
use self::observation::{ActiveCorrectionObservation, CorrectionObservationTarget, POLL_INTERVAL};

const FOCUS_SETTLE_DELAY: Duration = Duration::from_millis(80);
const PASTE_CONSUMPTION_DELAY: Duration = Duration::from_millis(300);
const COMMAND_QUEUE_CAPACITY: usize = 2;
const UIA_E_NOTSUPPORTED: HRESULT = HRESULT(0x80040200_u32 as i32);

#[derive(Clone)]
pub struct WindowsTextDeliverer {
    worker: Arc<DeliveryWorker>,
}

impl WindowsTextDeliverer {
    pub fn new() -> Result<Self, TextDeliveryError> {
        DeliveryWorker::spawn().map(|worker| Self {
            worker: Arc::new(worker),
        })
    }
}

impl fmt::Debug for WindowsTextDeliverer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WindowsTextDeliverer")
            .finish_non_exhaustive()
    }
}

impl TextDeliverer for WindowsTextDeliverer {
    fn authorization(&self) -> AccessibilityAuthorization {
        self.worker
            .request(|response| DeliveryCommand::Authorization { response })
            .unwrap_or(AccessibilityAuthorization::Denied)
    }

    fn request_authorization(&self) -> AccessibilityAuthorization {
        self.authorization()
    }

    fn target_privacy(&self) -> DeliveryTargetPrivacy {
        self.worker
            .request(|response| DeliveryCommand::TargetPrivacy { response })
            .unwrap_or(DeliveryTargetPrivacy::Sensitive)
    }

    fn deliver(&self, text: &str) -> Result<TextDeliveryOutcome, TextDeliveryError> {
        self.worker.request(|response| DeliveryCommand::Deliver {
            text: text.to_owned(),
            observer: None,
            response,
        })?
    }
}

impl CorrectionObservingTextDeliverer for WindowsTextDeliverer {
    fn deliver_and_observe(
        &self,
        text: &str,
        observer: TextEditObserver,
    ) -> Result<TextDeliveryOutcome, TextDeliveryError> {
        self.worker.request(|response| DeliveryCommand::Deliver {
            text: text.to_owned(),
            observer: Some(observer),
            response,
        })?
    }
}

pub fn copy_text_to_clipboard(text: &str) -> Result<(), TextDeliveryError> {
    clipboard::replace_with_text(text)
        .map(|_| ())
        .map_err(|failure| failure.error)
}

enum DeliveryCommand {
    Authorization {
        response: mpsc::Sender<AccessibilityAuthorization>,
    },
    TargetPrivacy {
        response: mpsc::Sender<DeliveryTargetPrivacy>,
    },
    Deliver {
        text: String,
        observer: Option<TextEditObserver>,
        response: mpsc::Sender<Result<TextDeliveryOutcome, TextDeliveryError>>,
    },
}

struct DeliveryWorker {
    sender: Mutex<Option<SyncSender<DeliveryCommand>>>,
    thread: Option<JoinHandle<()>>,
}

impl DeliveryWorker {
    fn spawn() -> Result<Self, TextDeliveryError> {
        let (sender, receiver) = mpsc::sync_channel(COMMAND_QUEUE_CAPACITY);
        let (initialization_sender, initialization_receiver) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("saymore-windows-text-delivery".to_owned())
            .spawn(move || run_worker(receiver, initialization_sender))
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

    fn request<T>(
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

fn run_worker(
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
    let mut active_observations = Vec::new();
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
            }) => match deliver_once(&runtime, &text) {
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
            },
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        active_observations.retain_mut(|observation| !observation.poll());
    }
}

fn observable_control_text(element: &IUIAutomationElement) -> Option<String> {
    if let Ok(Some(pattern)) =
        current_pattern::<IUIAutomationValuePattern>(element, UIA_ValuePatternId)
        && let Ok(value) = unsafe { pattern.CurrentValue() }
    {
        return Some(value.to_string());
    }
    if let Ok(Some(pattern)) =
        current_pattern::<IUIAutomationTextPattern>(element, UIA_TextPatternId)
        && let Ok(range) = unsafe { pattern.DocumentRange() }
        && let Ok(value) = unsafe { range.GetText(-1) }
    {
        return Some(value.to_string());
    }
    None
}

struct NativeDelivery {
    automation: IUIAutomation,
}

impl NativeDelivery {
    fn initialize() -> Result<Self, TextDeliveryError> {
        unsafe { OleInitialize(None) }
            .map_err(|error| system_error("initialize OLE delivery apartment", error))?;
        let automation = unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) };
        match automation {
            Ok(automation) => Ok(Self { automation }),
            Err(error) => {
                unsafe { OleUninitialize() };
                Err(system_error("create UI Automation client", error))
            }
        }
    }

    fn focused_target(&self) -> Result<FocusedTarget, TextDeliveryError> {
        let element = unsafe { self.automation.GetFocusedElement() }
            .map_err(|error| system_error("locate focused control", error))?;
        let root = unsafe { self.automation.GetRootElement() }
            .map_err(|error| system_error("locate desktop root", error))?;
        let is_root = unsafe { self.automation.CompareElements(&element, &root) }
            .map_err(|error| system_error("classify focused control", error))?
            .as_bool();
        if is_root {
            return Err(TextDeliveryError::NoFocusedControl);
        }

        let sensitive = sensitive_from_password_metadata(
            unsafe { element.CurrentIsPassword() }
                .ok()
                .map(|value| value.as_bool()),
        );
        let enabled = unsafe { element.CurrentIsEnabled() }
            .map_err(|error| target_error(sensitive, "inspect focused control", error))?
            .as_bool();
        let focusable = unsafe { element.CurrentIsKeyboardFocusable() }
            .map_err(|error| target_error(sensitive, "inspect focused control", error))?
            .as_bool();
        let editability = editability(&element);
        if target_action(TargetMetadata {
            enabled,
            focusable,
            editability,
        }) == TargetAction::Reject
        {
            return Err(TextDeliveryError::UnsupportedControl);
        }
        let initial_text = (!sensitive)
            .then(|| observable_control_text(&element))
            .flatten();
        Ok(FocusedTarget {
            element,
            sensitive,
            initial_text,
        })
    }

    fn focus_matches(&self, target: &FocusedTarget) -> Result<bool, TextDeliveryError> {
        let current = unsafe { self.automation.GetFocusedElement() }
            .map_err(|error| system_error("verify focused control", error))?;
        unsafe { self.automation.CompareElements(&target.element, &current) }
            .map(|same| same.as_bool())
            .map_err(|error| system_error("compare focused controls", error))
    }
}

impl Drop for NativeDelivery {
    fn drop(&mut self) {
        unsafe { OleUninitialize() };
    }
}

struct FocusedTarget {
    element: IUIAutomationElement,
    sensitive: bool,
    initial_text: Option<String>,
}

impl FocusedTarget {
    fn privacy(&self) -> DeliveryTargetPrivacy {
        if self.sensitive {
            DeliveryTargetPrivacy::Sensitive
        } else {
            DeliveryTargetPrivacy::Standard
        }
    }
}

fn sensitive_from_password_metadata(is_password: Option<bool>) -> bool {
    is_password.unwrap_or(true)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Editability {
    Writable,
    ReadOnly,
    Unsupported,
}

fn editability(element: &IUIAutomationElement) -> Editability {
    if let Ok(Some(pattern)) =
        current_pattern::<IUIAutomationValuePattern>(element, UIA_ValuePatternId)
        && let Ok(read_only) = unsafe { pattern.CurrentIsReadOnly() }
    {
        return if read_only.as_bool() {
            Editability::ReadOnly
        } else {
            Editability::Writable
        };
    }
    if matches!(
        current_pattern::<IUIAutomationTextEditPattern>(element, UIA_TextEditPatternId),
        Ok(Some(_))
    ) {
        Editability::Writable
    } else {
        Editability::Unsupported
    }
}

fn current_pattern<T: windows::core::Interface>(
    element: &IUIAutomationElement,
    pattern_id: windows::Win32::UI::Accessibility::UIA_PATTERN_ID,
) -> Result<Option<T>, TextDeliveryError> {
    match unsafe { element.GetCurrentPattern(pattern_id) } {
        Ok(pattern) => pattern
            .cast::<T>()
            .map(Some)
            .map_err(|error| system_error("cast focused control pattern", error)),
        Err(error) if error.code() == UIA_E_NOTSUPPORTED => Ok(None),
        Err(error) => Err(system_error("inspect focused control patterns", error)),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TargetMetadata {
    enabled: bool,
    focusable: bool,
    editability: Editability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TargetAction {
    Deliver,
    Reject,
}

fn target_action(metadata: TargetMetadata) -> TargetAction {
    if metadata.enabled && metadata.focusable && metadata.editability != Editability::ReadOnly {
        TargetAction::Deliver
    } else {
        TargetAction::Reject
    }
}

trait DeliveryOperations {
    type Target;
    type Snapshot;
    type Temporary;

    fn wait_for_focus(&self);
    fn focused_target(&self) -> Result<Self::Target, TextDeliveryError>;
    fn is_sensitive(&self, target: &Self::Target) -> bool;
    fn snapshot_clipboard(&self) -> Result<Self::Snapshot, TextDeliveryError>;
    fn set_temporary_text(
        &self,
        text: &str,
    ) -> Result<Self::Temporary, ClipboardSetupFailure<Self::Temporary>>;
    fn focus_matches(&self, target: &Self::Target) -> Result<bool, TextDeliveryError>;
    fn send_paste(&self) -> Result<(), TextDeliveryError>;
    fn wait_for_paste(&self);
    fn restore_clipboard(
        &self,
        snapshot: Self::Snapshot,
        temporary: Self::Temporary,
    ) -> Result<(), TextDeliveryError>;
}

struct ClipboardSetupFailure<T> {
    error: TextDeliveryError,
    temporary: Option<T>,
}

impl DeliveryOperations for NativeDelivery {
    type Target = FocusedTarget;
    type Snapshot = ClipboardSnapshot;
    type Temporary = TemporaryClipboard;

    fn wait_for_focus(&self) {
        thread::sleep(FOCUS_SETTLE_DELAY);
    }

    fn focused_target(&self) -> Result<Self::Target, TextDeliveryError> {
        self.focused_target()
    }

    fn is_sensitive(&self, target: &Self::Target) -> bool {
        target.sensitive
    }

    fn snapshot_clipboard(&self) -> Result<Self::Snapshot, TextDeliveryError> {
        clipboard::snapshot()
    }

    fn set_temporary_text(
        &self,
        text: &str,
    ) -> Result<Self::Temporary, ClipboardSetupFailure<Self::Temporary>> {
        clipboard::replace_with_text(text)
    }

    fn focus_matches(&self, target: &Self::Target) -> Result<bool, TextDeliveryError> {
        self.focus_matches(target)
    }

    fn send_paste(&self) -> Result<(), TextDeliveryError> {
        send_paste_shortcut()
    }

    fn wait_for_paste(&self) {
        thread::sleep(PASTE_CONSUMPTION_DELAY);
    }

    fn restore_clipboard(
        &self,
        snapshot: Self::Snapshot,
        temporary: Self::Temporary,
    ) -> Result<(), TextDeliveryError> {
        clipboard::restore_if_unchanged(snapshot, temporary)
    }
}

struct DeliveryAttempt<T> {
    outcome: TextDeliveryOutcome,
    target: T,
}

fn deliver_once<O: DeliveryOperations>(
    operations: &O,
    text: &str,
) -> Result<DeliveryAttempt<O::Target>, TextDeliveryError> {
    operations.wait_for_focus();
    let target = operations.focused_target()?;
    let sensitive = operations.is_sensitive(&target);
    let snapshot = operations
        .snapshot_clipboard()
        .map_err(|error| protect_sensitive_error(sensitive, error))?;
    let temporary = match operations.set_temporary_text(text) {
        Ok(temporary) => temporary,
        Err(failure) => {
            if let Some(temporary) = failure.temporary {
                let _ = operations.restore_clipboard(snapshot, temporary);
            }
            return Err(protect_sensitive_error(sensitive, failure.error));
        }
    };

    let paste_result = operations
        .focus_matches(&target)
        .and_then(|same| {
            if same {
                operations.send_paste()
            } else {
                Err(focus_changed_error())
            }
        })
        .and_then(|()| {
            operations.wait_for_paste();
            operations.focus_matches(&target).and_then(|same| {
                if same {
                    Ok(())
                } else {
                    Err(focus_changed_error())
                }
            })
        });
    let restore_result = operations.restore_clipboard(snapshot, temporary);
    match (paste_result, restore_result) {
        (_, Err(error)) => return Err(protect_sensitive_error(sensitive, error)),
        (Err(error), Ok(())) => return Err(protect_sensitive_error(sensitive, error)),
        (Ok(()), Ok(())) => {}
    }
    Ok(DeliveryAttempt {
        outcome: if sensitive {
            TextDeliveryOutcome::SecureClipboardAttempted
        } else {
            TextDeliveryOutcome::ClipboardAttempted
        },
        target,
    })
}

fn send_paste_shortcut() -> Result<(), TextDeliveryError> {
    let inputs = paste_inputs();
    let sent = unsafe { SendInput(&inputs, mem::size_of::<INPUT>() as i32) };
    if sent == inputs.len() as u32 {
        Ok(())
    } else {
        let cleanup = paste_cleanup_inputs(sent);
        if !cleanup.is_empty() {
            let _ = unsafe { SendInput(&cleanup, mem::size_of::<INPUT>() as i32) };
        }
        Err(TextDeliveryError::System(format!(
            "SendInput accepted {sent} of {} paste events",
            inputs.len()
        )))
    }
}

fn paste_cleanup_inputs(sent: u32) -> Vec<INPUT> {
    match sent {
        1 => vec![keyboard_input(VK_CONTROL, true)],
        2 => vec![keyboard_input(VK_V, true), keyboard_input(VK_CONTROL, true)],
        3 => vec![keyboard_input(VK_CONTROL, true)],
        _ => Vec::new(),
    }
}

fn paste_inputs() -> [INPUT; 4] {
    [
        keyboard_input(VK_CONTROL, false),
        keyboard_input(VK_V, false),
        keyboard_input(VK_V, true),
        keyboard_input(VK_CONTROL, true),
    ]
}

fn keyboard_input(key: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: 0,
                dwFlags: if up {
                    KEYEVENTF_KEYUP
                } else {
                    Default::default()
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn protect_sensitive_error(sensitive: bool, error: TextDeliveryError) -> TextDeliveryError {
    if !sensitive || matches!(error, TextDeliveryError::SecureDeliveryFailed(_)) {
        error
    } else {
        TextDeliveryError::SecureDeliveryFailed(error.to_string())
    }
}

fn target_error(sensitive: bool, operation: &str, error: WindowsError) -> TextDeliveryError {
    protect_sensitive_error(sensitive, system_error(operation, error))
}

fn focus_changed_error() -> TextDeliveryError {
    TextDeliveryError::System("focused control changed during text delivery".to_owned())
}

fn worker_unavailable() -> TextDeliveryError {
    TextDeliveryError::System("Windows text delivery worker is unavailable".to_owned())
}

fn system_error(operation: &str, error: WindowsError) -> TextDeliveryError {
    TextDeliveryError::System(format!("{operation} failed: {error}"))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[test]
    fn enabled_focusable_targets_use_clipboard_fallback_unless_read_only() {
        for (metadata, expected) in [
            (
                TargetMetadata {
                    enabled: true,
                    focusable: true,
                    editability: Editability::Writable,
                },
                TargetAction::Deliver,
            ),
            (
                TargetMetadata {
                    enabled: true,
                    focusable: true,
                    editability: Editability::ReadOnly,
                },
                TargetAction::Reject,
            ),
            (
                TargetMetadata {
                    enabled: true,
                    focusable: true,
                    editability: Editability::Unsupported,
                },
                TargetAction::Deliver,
            ),
            (
                TargetMetadata {
                    enabled: false,
                    focusable: true,
                    editability: Editability::Writable,
                },
                TargetAction::Reject,
            ),
        ] {
            assert_eq!(expected, target_action(metadata));
        }
    }

    #[test]
    fn password_and_unknown_password_metadata_are_sensitive() {
        assert!(!sensitive_from_password_metadata(Some(false)));
        assert!(sensitive_from_password_metadata(Some(true)));
        assert!(sensitive_from_password_metadata(None));
    }

    #[test]
    fn paste_sequence_releases_v_before_control() {
        let inputs = paste_inputs();
        let keys = inputs.map(|input| unsafe { input.Anonymous.ki });
        assert_eq!(VK_CONTROL, keys[0].wVk);
        assert_eq!(VK_V, keys[1].wVk);
        assert_eq!(VK_V, keys[2].wVk);
        assert_eq!(VK_CONTROL, keys[3].wVk);
        assert_eq!(0, keys[0].dwFlags.0);
        assert_eq!(0, keys[1].dwFlags.0);
        assert_eq!(KEYEVENTF_KEYUP, keys[2].dwFlags);
        assert_eq!(KEYEVENTF_KEYUP, keys[3].dwFlags);
    }

    #[test]
    fn partial_paste_injection_releases_only_pressed_keys() {
        let cleanup_keys = |sent| {
            paste_cleanup_inputs(sent)
                .into_iter()
                .map(|input| unsafe { input.Anonymous.ki })
                .map(|input| (input.wVk, input.dwFlags))
                .collect::<Vec<_>>()
        };
        assert!(cleanup_keys(0).is_empty());
        assert_eq!(vec![(VK_CONTROL, KEYEVENTF_KEYUP)], cleanup_keys(1));
        assert_eq!(
            vec![(VK_V, KEYEVENTF_KEYUP), (VK_CONTROL, KEYEVENTF_KEYUP)],
            cleanup_keys(2)
        );
        assert_eq!(vec![(VK_CONTROL, KEYEVENTF_KEYUP)], cleanup_keys(3));
        assert!(cleanup_keys(4).is_empty());
    }

    struct FakeOperations {
        sensitive: bool,
        focus_checks: Mutex<Vec<bool>>,
        paste_calls: AtomicUsize,
        restore_calls: AtomicUsize,
        setup_error: bool,
        restore_error: bool,
    }

    impl FakeOperations {
        fn new(sensitive: bool, focus_checks: Vec<bool>, restore_error: bool) -> Self {
            Self {
                sensitive,
                focus_checks: Mutex::new(focus_checks),
                paste_calls: AtomicUsize::new(0),
                restore_calls: AtomicUsize::new(0),
                setup_error: false,
                restore_error,
            }
        }

        fn with_setup_error(mut self) -> Self {
            self.setup_error = true;
            self
        }
    }

    impl DeliveryOperations for FakeOperations {
        type Target = ();
        type Snapshot = ();
        type Temporary = ();

        fn wait_for_focus(&self) {}

        fn focused_target(&self) -> Result<Self::Target, TextDeliveryError> {
            Ok(())
        }

        fn is_sensitive(&self, _target: &Self::Target) -> bool {
            self.sensitive
        }

        fn snapshot_clipboard(&self) -> Result<Self::Snapshot, TextDeliveryError> {
            Ok(())
        }

        fn set_temporary_text(
            &self,
            _text: &str,
        ) -> Result<Self::Temporary, ClipboardSetupFailure<Self::Temporary>> {
            if self.setup_error {
                Err(ClipboardSetupFailure {
                    error: TextDeliveryError::System("clipboard setup failed".to_owned()),
                    temporary: Some(()),
                })
            } else {
                Ok(())
            }
        }

        fn focus_matches(&self, _target: &Self::Target) -> Result<bool, TextDeliveryError> {
            self.focus_checks
                .lock()
                .map_err(|_| worker_unavailable())?
                .pop()
                .ok_or_else(worker_unavailable)
        }

        fn send_paste(&self) -> Result<(), TextDeliveryError> {
            self.paste_calls.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn wait_for_paste(&self) {}

        fn restore_clipboard(
            &self,
            _snapshot: Self::Snapshot,
            _temporary: Self::Temporary,
        ) -> Result<(), TextDeliveryError> {
            self.restore_calls.fetch_add(1, Ordering::Relaxed);
            if self.restore_error {
                Err(TextDeliveryError::System("restore failed".to_owned()))
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn focus_change_before_paste_does_not_send_and_still_restores() {
        let operations = FakeOperations::new(false, vec![false], false);
        assert!(matches!(
            deliver_once(&operations, "private text"),
            Err(TextDeliveryError::System(_))
        ));
        assert_eq!(0, operations.paste_calls.load(Ordering::Relaxed));
        assert_eq!(1, operations.restore_calls.load(Ordering::Relaxed));
    }

    #[test]
    fn clipboard_setup_failure_after_mutation_still_restores_without_pasting() {
        let operations = FakeOperations::new(false, Vec::new(), false).with_setup_error();
        assert!(deliver_once(&operations, "private text").is_err());
        assert_eq!(0, operations.paste_calls.load(Ordering::Relaxed));
        assert_eq!(1, operations.restore_calls.load(Ordering::Relaxed));
    }

    #[test]
    fn focus_change_after_paste_never_repeats_paste() {
        let operations = FakeOperations::new(false, vec![false, true], false);
        assert!(deliver_once(&operations, "private text").is_err());
        assert_eq!(1, operations.paste_calls.load(Ordering::Relaxed));
        assert_eq!(1, operations.restore_calls.load(Ordering::Relaxed));
    }

    #[test]
    fn standard_and_sensitive_attempts_have_distinct_outcomes() {
        let standard = FakeOperations::new(false, vec![true, true], false);
        let sensitive = FakeOperations::new(true, vec![true, true], false);
        assert_eq!(
            Ok(TextDeliveryOutcome::ClipboardAttempted),
            deliver_once(&standard, "private text").map(|attempt| attempt.outcome)
        );
        assert_eq!(
            Ok(TextDeliveryOutcome::SecureClipboardAttempted),
            deliver_once(&sensitive, "private text").map(|attempt| attempt.outcome)
        );
    }

    #[test]
    fn sensitive_restore_failure_uses_secure_error_without_text() {
        let operations = FakeOperations::new(true, vec![true, true], true);
        let error = deliver_once(&operations, "do not disclose this").err();
        assert!(matches!(
            error,
            Some(TextDeliveryError::SecureDeliveryFailed(_))
        ));
        assert!(!format!("{error:?}").contains("do not disclose this"));
        assert_eq!(1, operations.paste_calls.load(Ordering::Relaxed));
        assert_eq!(1, operations.restore_calls.load(Ordering::Relaxed));
    }

    #[test]
    fn restore_failure_takes_priority_over_an_earlier_paste_failure() {
        let operations = FakeOperations::new(false, vec![false], true);
        let error = deliver_once(&operations, "private text");
        assert!(matches!(
            error,
            Err(TextDeliveryError::System(message)) if message == "restore failed"
        ));
    }
}
