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
    Input::KeyboardAndMouse::{
        HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, RegisterHotKey,
        UnregisterHotKey,
    },
    WindowsAndMessaging::{MSG, PM_NOREMOVE, PM_REMOVE, PeekMessageW, WM_HOTKEY},
};

use crate::DictationShortcutAction;
use crate::windows_right_alt_hook::WindowsRightAltHook;
use crate::windows_shortcut_capture::capture_shortcut;
use crate::windows_shortcut_registry::{HotKeyRegistry, RegisteredShortcuts};

const CANCEL_ID: i32 = 0x5fff;
const ESCAPE_VK: u32 = 0x1b;
const POLL_INTERVAL: Duration = Duration::from_millis(10);
const STORAGE_PREFIX: &str = "windows:";

const VK_BACK: u32 = 0x08;
const VK_TAB: u32 = 0x09;
const VK_RETURN: u32 = 0x0d;
const VK_SHIFT: u32 = 0x10;
const VK_CONTROL: u32 = 0x11;
const VK_MENU: u32 = 0x12;
const VK_SPACE: u32 = 0x20;
const VK_PRIOR: u32 = 0x21;
const VK_NEXT: u32 = 0x22;
const VK_END: u32 = 0x23;
const VK_HOME: u32 = 0x24;
const VK_LEFT: u32 = 0x25;
const VK_UP: u32 = 0x26;
const VK_RIGHT: u32 = 0x27;
const VK_DOWN: u32 = 0x28;
const VK_INSERT: u32 = 0x2d;
const VK_DELETE: u32 = 0x2e;
const VK_LWIN: u32 = 0x5b;
const VK_RWIN: u32 = 0x5c;
const VK_RMENU: u32 = 0xa5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowsShortcut {
    modifiers: u32,
    virtual_key: u32,
}

impl Default for WindowsShortcut {
    fn default() -> Self {
        Self {
            modifiers: 0,
            virtual_key: VK_RMENU,
        }
    }
}

impl WindowsShortcut {
    pub fn from_storage_value(value: &str) -> Result<Self, WindowsShortcutError> {
        let value = value
            .trim()
            .to_ascii_lowercase()
            .strip_prefix(STORAGE_PREFIX)
            .map(str::to_owned)
            .ok_or(WindowsShortcutError::InvalidStorageValue)?;
        let mut modifiers = 0;
        let mut key = None;
        for part in value.split('+') {
            match part {
                "control" => modifiers |= MOD_CONTROL.0,
                "alt" => modifiers |= MOD_ALT.0,
                "shift" => modifiers |= MOD_SHIFT.0,
                "windows" => modifiers |= MOD_WIN.0,
                part if key.is_none() => key = parse_virtual_key(part),
                _ => return Err(WindowsShortcutError::InvalidStorageValue),
            }
        }
        Self::new(
            modifiers,
            key.ok_or(WindowsShortcutError::InvalidStorageValue)?,
        )
    }

    pub fn from_capture(
        key: &str,
        control: bool,
        alt: bool,
        shift: bool,
        windows: bool,
    ) -> Result<Self, WindowsShortcutError> {
        let mut modifiers = 0;
        if control {
            modifiers |= MOD_CONTROL.0;
        }
        if alt {
            modifiers |= MOD_ALT.0;
        }
        if shift {
            modifiers |= MOD_SHIFT.0;
        }
        if windows {
            modifiers |= MOD_WIN.0;
        }
        let key = parse_virtual_key(&key.trim().to_ascii_lowercase())
            .ok_or(WindowsShortcutError::InvalidStorageValue)?;
        Self::new(modifiers, key)
    }

    pub(super) fn new(modifiers: u32, virtual_key: u32) -> Result<Self, WindowsShortcutError> {
        if virtual_key == VK_RMENU && modifiers == 0 {
            return Ok(Self::default());
        }
        if modifiers == 0 {
            return Err(WindowsShortcutError::MissingModifier);
        }
        if is_modifier_key(virtual_key) || key_label(virtual_key).is_none() {
            return Err(WindowsShortcutError::InvalidStorageValue);
        }
        let shortcut = Self {
            modifiers,
            virtual_key,
        };
        if shortcut.is_system_reserved() {
            return Err(WindowsShortcutError::SystemReserved);
        }
        Ok(shortcut)
    }

    pub fn storage_value(self) -> String {
        if self.is_right_alt() {
            return "windows:right-alt".to_owned();
        }
        let mut parts = Vec::with_capacity(5);
        if self.modifiers & MOD_CONTROL.0 != 0 {
            parts.push("control");
        }
        if self.modifiers & MOD_ALT.0 != 0 {
            parts.push("alt");
        }
        if self.modifiers & MOD_SHIFT.0 != 0 {
            parts.push("shift");
        }
        if self.modifiers & MOD_WIN.0 != 0 {
            parts.push("windows");
        }
        parts.push(key_label(self.virtual_key).unwrap_or("invalid"));
        format!("{STORAGE_PREFIX}{}", parts.join("+"))
    }

    pub fn display_label(self) -> String {
        if self.is_right_alt() {
            return "Right Alt".to_owned();
        }
        self.storage_value()
            .trim_start_matches(STORAGE_PREFIX)
            .split('+')
            .map(display_part)
            .collect::<Vec<_>>()
            .join(" + ")
    }

    pub fn likely_system_conflict(self) -> bool {
        let ctrl_shift = MOD_CONTROL.0 | MOD_SHIFT.0;
        self.modifiers == ctrl_shift
            || (self.modifiers == MOD_CONTROL.0 && self.virtual_key == VK_SPACE)
    }

    pub(super) fn is_right_alt(self) -> bool {
        self.modifiers == 0 && self.virtual_key == VK_RMENU
    }

    fn is_system_reserved(self) -> bool {
        let win = self.modifiers & MOD_WIN.0 != 0;
        let alt = self.modifiers & MOD_ALT.0 != 0;
        let ctrl = self.modifiers & MOD_CONTROL.0 != 0;
        (win && matches!(
            self.virtual_key,
            0x48 | 0x51 | 0x53 | 0x56 | VK_SPACE | VK_LEFT | VK_UP | VK_RIGHT | VK_DOWN
        )) || (alt && self.virtual_key == VK_TAB)
            || (alt && self.virtual_key == 0x73)
            || (ctrl && alt && self.virtual_key == VK_DELETE)
    }

    fn registration_modifiers(self) -> HOT_KEY_MODIFIERS {
        HOT_KEY_MODIFIERS(self.modifiers | MOD_NOREPEAT.0)
    }
}

fn parse_virtual_key(value: &str) -> Option<u32> {
    match value {
        "right-alt" => Some(VK_RMENU),
        "backspace" => Some(VK_BACK),
        "tab" => Some(VK_TAB),
        "enter" => Some(VK_RETURN),
        "space" => Some(VK_SPACE),
        "page-up" => Some(VK_PRIOR),
        "page-down" => Some(VK_NEXT),
        "end" => Some(VK_END),
        "home" => Some(VK_HOME),
        "left" => Some(VK_LEFT),
        "up" => Some(VK_UP),
        "right" => Some(VK_RIGHT),
        "down" => Some(VK_DOWN),
        "insert" => Some(VK_INSERT),
        "delete" => Some(VK_DELETE),
        _ => parse_alphanumeric_or_function_key(value),
    }
}

fn parse_alphanumeric_or_function_key(value: &str) -> Option<u32> {
    if let Some(number) = value
        .strip_prefix('f')
        .and_then(|value| value.parse::<u32>().ok())
        && (1..=24).contains(&number)
    {
        return Some(0x70 + number - 1);
    }
    let mut characters = value.chars();
    let character = characters.next()?;
    if characters.next().is_none() && character.is_ascii_alphanumeric() {
        return Some(character.to_ascii_uppercase() as u32);
    }
    None
}

pub(super) fn key_label(virtual_key: u32) -> Option<&'static str> {
    match virtual_key {
        VK_BACK => Some("backspace"),
        VK_TAB => Some("tab"),
        VK_RETURN => Some("enter"),
        VK_SPACE => Some("space"),
        VK_PRIOR => Some("page-up"),
        VK_NEXT => Some("page-down"),
        VK_END => Some("end"),
        VK_HOME => Some("home"),
        VK_LEFT => Some("left"),
        VK_UP => Some("up"),
        VK_RIGHT => Some("right"),
        VK_DOWN => Some("down"),
        VK_INSERT => Some("insert"),
        VK_DELETE => Some("delete"),
        0x30..=0x39 => Some(DIGIT_LABELS[(virtual_key - 0x30) as usize]),
        0x41..=0x5a => Some(LETTER_LABELS[(virtual_key - 0x41) as usize]),
        0x70..=0x87 => Some(FUNCTION_LABELS[(virtual_key - 0x70) as usize]),
        _ => None,
    }
}

const DIGIT_LABELS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
const LETTER_LABELS: [&str; 26] = [
    "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s",
    "t", "u", "v", "w", "x", "y", "z",
];
const FUNCTION_LABELS: [&str; 24] = [
    "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9", "f10", "f11", "f12", "f13", "f14", "f15",
    "f16", "f17", "f18", "f19", "f20", "f21", "f22", "f23", "f24",
];

fn display_part(value: &str) -> String {
    match value {
        "control" => "Ctrl".to_owned(),
        "windows" => "Win".to_owned(),
        "page-up" => "Page Up".to_owned(),
        "page-down" => "Page Down".to_owned(),
        _ => {
            let mut characters = value.chars();
            characters.next().map_or_else(String::new, |first| {
                first.to_ascii_uppercase().to_string() + characters.as_str()
            })
        }
    }
}

pub(super) fn is_modifier_key(key: u32) -> bool {
    matches!(key, VK_SHIFT | VK_CONTROL | VK_MENU | VK_LWIN | VK_RWIN)
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
                shortcuts: RwLock::new(normalize_shortcuts(shortcuts)),
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

fn normalize_shortcuts(shortcuts: Vec<WindowsShortcut>) -> Vec<WindowsShortcut> {
    if shortcuts.is_empty() {
        vec![WindowsShortcut::default()]
    } else {
        shortcuts
    }
}

pub(super) fn validate_collection(
    shortcuts: &[WindowsShortcut],
) -> Result<(), WindowsShortcutError> {
    if shortcuts.is_empty() {
        return Err(WindowsShortcutError::InvalidStorageValue);
    }
    if shortcuts.iter().copied().collect::<HashSet<_>>().len() != shortcuts.len() {
        return Err(WindowsShortcutError::Duplicate);
    }
    Ok(())
}

struct Win32HotKeyRegistry {
    right_alt_ids: HashSet<i32>,
    right_alt_hook: Option<WindowsRightAltHook>,
    right_alt_events: mpsc::SyncSender<()>,
}

impl Win32HotKeyRegistry {
    fn new(right_alt_events: mpsc::SyncSender<()>) -> Self {
        Self {
            right_alt_ids: HashSet::new(),
            right_alt_hook: None,
            right_alt_events,
        }
    }
}

impl HotKeyRegistry for Win32HotKeyRegistry {
    fn register(&mut self, id: i32, shortcut: WindowsShortcut) -> Result<(), WindowsShortcutError> {
        if shortcut.is_right_alt() {
            if self.right_alt_hook.is_none() {
                self.right_alt_hook =
                    Some(WindowsRightAltHook::install(self.right_alt_events.clone())?);
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
    on_action: impl Fn(DictationShortcutAction),
    ready: mpsc::SyncSender<Result<(), WindowsShortcutError>>,
) {
    let mut queue_message = MSG::default();
    let _ = unsafe { PeekMessageW(&mut queue_message, None, 0, 0, PM_NOREMOVE) };
    let (right_alt_events, received_right_alt_events) = mpsc::sync_channel(8);
    let registry = Win32HotKeyRegistry::new(right_alt_events);
    let mut registrations = match RegisteredShortcuts::new(registry, &shortcuts) {
        Ok(registrations) => registrations,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    let _ = ready.send(Ok(()));
    let mut cancel_registered = false;
    loop {
        match receiver.recv_timeout(POLL_INTERVAL) {
            Ok(MonitorCommand::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Ok(command) => handle_monitor_command(command, &state, &mut registrations),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        let capture_active = state.capture_active.load(Ordering::Acquire);
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
    registrations.shutdown();
    if cancel_registered {
        let _ = unsafe { UnregisterHotKey(None, CANCEL_ID) };
    }
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
