use std::{
    collections::HashSet,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use thiserror::Error;
use windows::Win32::UI::{
    Input::KeyboardAndMouse::{HOT_KEY_MODIFIERS, MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey},
    WindowsAndMessaging::{MSG, PM_NOREMOVE, PM_REMOVE, PeekMessageW, WM_HOTKEY},
};

mod shortcut_value;

pub use shortcut_value::WindowsShortcut;
pub(super) use shortcut_value::{is_modifier_key, key_label};

use crate::DictationShortcutAction;
use crate::windows_right_alt_hook::WindowsRightAltHook;
use crate::windows_shortcut_capture::capture_shortcut;
use crate::windows_shortcut_registry::{HotKeyRegistry, RegisteredShortcuts};

const CANCEL_ID: i32 = 0x5fff;
const ESCAPE_VK: u32 = 0x1b;
const POLL_INTERVAL: Duration = Duration::from_millis(10);

impl WindowsShortcut {
    fn registration_modifiers(self) -> HOT_KEY_MODIFIERS {
        HOT_KEY_MODIFIERS(self.modifiers | MOD_NOREPEAT.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WindowsShortcutError {
    #[error("the saved Windows shortcut is invalid")]
    InvalidStorageValue,
    #[error("the Windows shortcut requires Ctrl, Alt, Shift, or Win")]
    MissingModifier,
    #[error("this shortcut is reserved by Windows")]
    SystemReserved,
    #[error("this shortcut is already configured")]
    Duplicate,
    #[error("the shortcut state is unavailable")]
    StateUnavailable,
    #[error("another shortcut capture is already active")]
    CaptureActive,
    #[error("shortcut capture was cancelled")]
    CaptureCancelled,
    #[error("another shortcut update is awaiting persistence")]
    UpdateActive,
    #[error("Windows could not register {shortcut}: {reason}")]
    RegistrationConflict { shortcut: String, reason: String },
    #[error("the shortcut monitor is shutting down")]
    RuntimeClosed,
    #[error("the shortcut monitor thread could not start")]
    ThreadStart,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowsShortcutUpdate(u64);

enum MonitorCommand {
    Replace {
        shortcuts: Vec<WindowsShortcut>,
        reply: mpsc::SyncSender<Result<(), WindowsShortcutError>>,
    },
    Stage {
        id: u64,
        shortcuts: Vec<WindowsShortcut>,
        reply: mpsc::SyncSender<Result<(), WindowsShortcutError>>,
    },
    Finish {
        id: u64,
        commit: bool,
        reply: mpsc::SyncSender<Result<(), WindowsShortcutError>>,
    },
    Shutdown,
}

struct ControllerState {
    shortcuts: RwLock<Vec<WindowsShortcut>>,
    sender: Mutex<Option<mpsc::Sender<MonitorCommand>>>,
    capture_active: AtomicBool,
    runtime_closed: AtomicBool,
    next_update_id: AtomicU64,
}

#[derive(Clone)]
pub struct WindowsShortcutController {
    state: Arc<ControllerState>,
}

impl WindowsShortcutController {
    pub fn new(shortcuts: Vec<WindowsShortcut>) -> Self {
        Self {
            state: Arc::new(ControllerState {
                shortcuts: RwLock::new(shortcuts),
                sender: Mutex::new(None),
                capture_active: AtomicBool::new(false),
                runtime_closed: AtomicBool::new(false),
                next_update_id: AtomicU64::new(1),
            }),
        }
    }

    pub fn current(&self) -> Result<Vec<WindowsShortcut>, WindowsShortcutError> {
        self.state
            .shortcuts
            .read()
            .map(|shortcuts| shortcuts.clone())
            .map_err(|_| WindowsShortcutError::StateUnavailable)
    }

    pub fn replace(&self, shortcuts: Vec<WindowsShortcut>) -> Result<(), WindowsShortcutError> {
        validate_collection(&shortcuts)?;
        if let Some(sender) = self.sender()? {
            request_monitor(&sender, |reply| MonitorCommand::Replace {
                shortcuts,
                reply,
            })?;
        } else {
            self.store_current(shortcuts)?;
        }
        Ok(())
    }

    pub fn stage_replace(
        &self,
        shortcuts: Vec<WindowsShortcut>,
    ) -> Result<WindowsShortcutUpdate, WindowsShortcutError> {
        validate_collection(&shortcuts)?;
        let sender = self.sender()?.ok_or(WindowsShortcutError::RuntimeClosed)?;
        let id = self.state.next_update_id.fetch_add(1, Ordering::Relaxed);
        request_monitor(&sender, |reply| MonitorCommand::Stage {
            id,
            shortcuts,
            reply,
        })?;
        Ok(WindowsShortcutUpdate(id))
    }

    pub fn commit(&self, update: WindowsShortcutUpdate) -> Result<(), WindowsShortcutError> {
        self.finish_update(update, true)
    }

    pub fn rollback(&self, update: WindowsShortcutUpdate) -> Result<(), WindowsShortcutError> {
        self.finish_update(update, false)
    }

    fn finish_update(
        &self,
        update: WindowsShortcutUpdate,
        commit: bool,
    ) -> Result<(), WindowsShortcutError> {
        let sender = self.sender()?.ok_or(WindowsShortcutError::RuntimeClosed)?;
        request_monitor(&sender, |reply| MonitorCommand::Finish {
            id: update.0,
            commit,
            reply,
        })
    }

    pub fn capture(&self) -> Result<WindowsShortcut, WindowsShortcutError> {
        capture_shortcut(&self.state.capture_active, &self.state.runtime_closed)
    }

    fn sender(&self) -> Result<Option<mpsc::Sender<MonitorCommand>>, WindowsShortcutError> {
        self.state
            .sender
            .lock()
            .map(|sender| sender.clone())
            .map_err(|_| WindowsShortcutError::StateUnavailable)
    }

    fn store_current(&self, shortcuts: Vec<WindowsShortcut>) -> Result<(), WindowsShortcutError> {
        self.state
            .shortcuts
            .write()
            .map(|mut current| *current = shortcuts)
            .map_err(|_| WindowsShortcutError::StateUnavailable)
    }
}

fn request_monitor(
    sender: &mpsc::Sender<MonitorCommand>,
    command: impl FnOnce(mpsc::SyncSender<Result<(), WindowsShortcutError>>) -> MonitorCommand,
) -> Result<(), WindowsShortcutError> {
    let (reply, result) = mpsc::sync_channel(1);
    sender
        .send(command(reply))
        .map_err(|_| WindowsShortcutError::RuntimeClosed)?;
    result
        .recv()
        .map_err(|_| WindowsShortcutError::RuntimeClosed)?
}

pub(super) fn validate_collection(
    shortcuts: &[WindowsShortcut],
) -> Result<(), WindowsShortcutError> {
    if shortcuts.iter().copied().collect::<HashSet<_>>().len() != shortcuts.len() {
        return Err(WindowsShortcutError::Duplicate);
    }
    Ok(())
}

struct Win32HotKeyRegistry {
    right_alt_ids: HashSet<i32>,
    right_alt_hook: Option<WindowsRightAltHook>,
    right_alt_events: mpsc::SyncSender<()>,
    right_alt_enabled: Arc<AtomicBool>,
}

impl Win32HotKeyRegistry {
    fn new(right_alt_events: mpsc::SyncSender<()>, right_alt_enabled: Arc<AtomicBool>) -> Self {
        Self {
            right_alt_ids: HashSet::new(),
            right_alt_hook: None,
            right_alt_events,
            right_alt_enabled,
        }
    }
}

impl HotKeyRegistry for Win32HotKeyRegistry {
    fn register(&mut self, id: i32, shortcut: WindowsShortcut) -> Result<(), WindowsShortcutError> {
        if shortcut.is_right_alt() {
            if self.right_alt_hook.is_none() {
                self.right_alt_hook = Some(WindowsRightAltHook::install(
                    self.right_alt_events.clone(),
                    Arc::clone(&self.right_alt_enabled),
                )?);
            }
            self.right_alt_ids.insert(id);
            return Ok(());
        }
        unsafe {
            RegisterHotKey(
                None,
                id,
                shortcut.registration_modifiers(),
                shortcut.virtual_key,
            )
        }
        .map_err(|error| WindowsShortcutError::RegistrationConflict {
            shortcut: shortcut.display_label(),
            reason: error.to_string(),
        })
    }

    fn unregister(&mut self, id: i32) {
        if self.right_alt_ids.remove(&id) {
            if self.right_alt_ids.is_empty() {
                self.right_alt_hook.take();
            }
            return;
        }
        let _ = unsafe { UnregisterHotKey(None, id) };
    }
}

pub struct WindowsShortcutMonitor {
    sender: mpsc::Sender<MonitorCommand>,
    state: Arc<ControllerState>,
    worker: Option<JoinHandle<()>>,
}

impl WindowsShortcutMonitor {
    pub fn start(
        is_recording: Arc<dyn Fn() -> bool + Send + Sync>,
        shortcuts_enabled: Arc<dyn Fn() -> bool + Send + Sync>,
        controller: WindowsShortcutController,
        on_action: impl Fn(DictationShortcutAction) + Send + 'static,
    ) -> Result<Self, WindowsShortcutError> {
        let shortcuts = controller.current()?;
        let (sender, receiver) = mpsc::channel();
        let (ready, started) = mpsc::sync_channel(1);
        let state = Arc::clone(&controller.state);
        let worker_state = Arc::clone(&state);
        let worker = thread::Builder::new()
            .name("saymore-windows-shortcuts".to_owned())
            .spawn(move || {
                monitor_loop(
                    worker_state,
                    shortcuts,
                    receiver,
                    is_recording,
                    shortcuts_enabled,
                    on_action,
                    ready,
                )
            })
            .map_err(|_| WindowsShortcutError::ThreadStart)?;
        started
            .recv()
            .map_err(|_| WindowsShortcutError::ThreadStart)??;
        state
            .sender
            .lock()
            .map_err(|_| WindowsShortcutError::StateUnavailable)?
            .replace(sender.clone());
        Ok(Self {
            sender,
            state,
            worker: Some(worker),
        })
    }

    pub fn shutdown(&mut self) {
        self.state.runtime_closed.store(true, Ordering::Release);
        let _ = self.sender.send(MonitorCommand::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        if let Ok(mut sender) = self.state.sender.lock() {
            sender.take();
        }
    }
}

impl Drop for WindowsShortcutMonitor {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn monitor_loop(
    state: Arc<ControllerState>,
    shortcuts: Vec<WindowsShortcut>,
    receiver: mpsc::Receiver<MonitorCommand>,
    is_recording: Arc<dyn Fn() -> bool + Send + Sync>,
    shortcuts_enabled: Arc<dyn Fn() -> bool + Send + Sync>,
    on_action: impl Fn(DictationShortcutAction),
    ready: mpsc::SyncSender<Result<(), WindowsShortcutError>>,
) {
    let mut queue_message = MSG::default();
    let _ = unsafe { PeekMessageW(&mut queue_message, None, 0, 0, PM_NOREMOVE) };
    let (right_alt_events, received_right_alt_events) = mpsc::sync_channel(8);
    let right_alt_enabled = Arc::new(AtomicBool::new(false));
    let registry = Win32HotKeyRegistry::new(right_alt_events, Arc::clone(&right_alt_enabled));
    let mut registrations = match RegisteredShortcuts::new(registry, &shortcuts) {
        Ok(registrations) => registrations,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    update_right_alt_enabled(
        &right_alt_enabled,
        &registrations,
        state.capture_active.load(Ordering::Acquire),
        shortcuts_enabled.as_ref(),
    );
    let _ = ready.send(Ok(()));
    let mut cancel_registered = false;
    loop {
        match receiver.recv_timeout(POLL_INTERVAL) {
            Ok(MonitorCommand::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Ok(command) => handle_monitor_command(command, &state, &mut registrations),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        let capture_active = state.capture_active.load(Ordering::Acquire);
        update_right_alt_enabled(
            &right_alt_enabled,
            &registrations,
            capture_active,
            shortcuts_enabled.as_ref(),
        );
        let should_register_cancel = is_recording() && !capture_active;
        if should_register_cancel != cancel_registered {
            cancel_registered = update_cancel_registration(should_register_cancel);
        }
        process_messages(
            &registrations.active_ids(),
            cancel_registered,
            capture_active,
            &on_action,
        );
        while received_right_alt_events.try_recv().is_ok() {
            if !capture_active && registrations.active_contains(WindowsShortcut::default()) {
                on_action(DictationShortcutAction::Toggle);
            }
        }
    }
    right_alt_enabled.store(false, Ordering::Release);
    registrations.shutdown();
    if cancel_registered {
        let _ = unsafe { UnregisterHotKey(None, CANCEL_ID) };
    }
}

fn update_right_alt_enabled(
    enabled: &AtomicBool,
    registrations: &RegisteredShortcuts<Win32HotKeyRegistry>,
    capture_active: bool,
    shortcuts_enabled: &(dyn Fn() -> bool + Send + Sync),
) {
    enabled.store(
        !capture_active
            && shortcuts_enabled()
            && registrations.active_contains(WindowsShortcut::default()),
        Ordering::Release,
    );
}

fn handle_monitor_command(
    command: MonitorCommand,
    state: &ControllerState,
    registrations: &mut RegisteredShortcuts<Win32HotKeyRegistry>,
) {
    match command {
        MonitorCommand::Replace { shortcuts, reply } => {
            let result = registrations
                .replace(&shortcuts)
                .and_then(|()| store_shortcuts(state, shortcuts));
            let _ = reply.send(result);
        }
        MonitorCommand::Stage {
            id,
            shortcuts,
            reply,
        } => {
            let result = registrations
                .stage(id, &shortcuts)
                .and_then(|()| store_shortcuts(state, shortcuts));
            let _ = reply.send(result);
        }
        MonitorCommand::Finish { id, commit, reply } => {
            let previous = registrations
                .pending
                .as_ref()
                .map(|pending| pending.previous.iter().map(|item| item.shortcut).collect());
            let result = registrations.finish(id, commit).and_then(|()| {
                if commit {
                    Ok(())
                } else {
                    store_shortcuts(state, previous.unwrap_or_default())
                }
            });
            let _ = reply.send(result);
        }
        MonitorCommand::Shutdown => {}
    }
}

fn store_shortcuts(
    state: &ControllerState,
    shortcuts: Vec<WindowsShortcut>,
) -> Result<(), WindowsShortcutError> {
    state
        .shortcuts
        .write()
        .map(|mut current| *current = shortcuts)
        .map_err(|_| WindowsShortcutError::StateUnavailable)
}

fn update_cancel_registration(register: bool) -> bool {
    if register {
        unsafe { RegisterHotKey(None, CANCEL_ID, MOD_NOREPEAT, ESCAPE_VK) }
            .inspect_err(
                |error| tracing::warn!(event = "shortcut.cancel_register_failed", reason = %error),
            )
            .is_ok()
    } else {
        let _ = unsafe { UnregisterHotKey(None, CANCEL_ID) };
        false
    }
}

fn process_messages(
    toggle_ids: &HashSet<i32>,
    cancel_registered: bool,
    capture_active: bool,
    on_action: &impl Fn(DictationShortcutAction),
) {
    let mut message = MSG::default();
    while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() } {
        if message.message != WM_HOTKEY {
            continue;
        }
        if capture_active {
            continue;
        }
        let Ok(id) = i32::try_from(message.wParam.0) else {
            continue;
        };
        if cancel_registered && id == CANCEL_ID {
            on_action(DictationShortcutAction::Cancel);
        } else if toggle_ids.contains(&id) {
            on_action(DictationShortcutAction::Toggle);
        }
    }
}
