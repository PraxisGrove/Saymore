use accessibility_sys::AXIsProcessTrusted;
use core_foundation::runloop::{CFRunLoop, kCFRunLoopCommonModes};
use core_graphics::event::{
    CGEvent, CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventType, CallbackResult, EventField,
};
use std::{
    collections::HashSet,
    sync::{
        Arc, Mutex, RwLock,
        mpsc::{Receiver, Sender, channel},
    },
    thread,
    time::Duration,
};
use thiserror::Error;

use crate::DictationShortcutAction;

mod event_tap;
mod key_mapping;
mod untrusted_poll;

#[cfg(test)]
use event_tap::{ModifierState, handle_modifier_event, mark_active_modifiers_used};
use key_mapping::{key_code_for_character, key_label, modifier_label};

const PERMISSION_RETRY_INTERVAL: Duration = Duration::from_secs(1);
const RIGHT_COMMAND_KEY_CODE: i64 = 54;
const ESCAPE_KEY_CODE: i64 = 53;
type ShortcutCaptureResult = Result<MacOsShortcut, MacOsShortcutError>;
type ShortcutCaptureSender = Sender<ShortcutCaptureResult>;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MacOsShortcutError {
    #[error("this shortcut is already configured")]
    Duplicate,
    #[error("this shortcut requires Command, Control, Option, or Fn")]
    MissingModifier,
    #[error("this shortcut is reserved by macOS")]
    SystemReserved,
    #[error("the saved shortcut is invalid")]
    InvalidStorageValue,
    #[error("the shortcut state is unavailable")]
    StateUnavailable,
    #[error("another shortcut capture is already active")]
    CaptureActive,
    #[error("shortcut capture was cancelled")]
    CaptureCancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacOsShortcut {
    key: ShortcutKey,
    function: bool,
    command: bool,
    control: bool,
    option: bool,
    shift: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShortcutKey {
    Modifier(i64),
    Physical(i64),
}

impl Default for MacOsShortcut {
    fn default() -> Self {
        Self::modifier(RIGHT_COMMAND_KEY_CODE)
    }
}

impl MacOsShortcut {
    fn modifier(key_code: i64) -> Self {
        Self {
            key: ShortcutKey::Modifier(key_code),
            function: false,
            command: false,
            control: false,
            option: false,
            shift: false,
        }
    }

    fn physical(key_code: i64, flags: CGEventFlags) -> Self {
        Self {
            key: ShortcutKey::Physical(key_code),
            function: flags.contains(CGEventFlags::CGEventFlagSecondaryFn),
            command: flags.contains(CGEventFlags::CGEventFlagCommand),
            control: flags.contains(CGEventFlags::CGEventFlagControl),
            option: flags.contains(CGEventFlags::CGEventFlagAlternate),
            shift: flags.contains(CGEventFlags::CGEventFlagShift),
        }
    }

    pub fn from_capture(
        text: &str,
        command: bool,
        control: bool,
        option: bool,
        shift: bool,
    ) -> Result<Self, MacOsShortcutError> {
        let normalized = text.trim().to_ascii_uppercase();
        if normalized == "FN" {
            return Ok(Self::modifier(63));
        }
        let key_code = if text == " " || normalized == "SPACE" {
            49
        } else {
            normalized
                .chars()
                .next()
                .filter(|_| normalized.chars().count() == 1)
                .and_then(key_code_for_character)
                .ok_or(MacOsShortcutError::InvalidStorageValue)?
        };
        Self {
            key: ShortcutKey::Physical(key_code),
            function: false,
            command,
            control,
            option,
            shift,
        }
        .validate()
    }

    pub fn from_storage_value(value: &str) -> Result<Self, MacOsShortcutError> {
        if value == "right-command" {
            return Ok(Self::default());
        }
        if value == "fn" {
            return Ok(Self::modifier(63));
        }
        if let Some(code) = value.strip_prefix("modifier-") {
            return code
                .parse::<i64>()
                .map(Self::modifier)
                .map_err(|_| MacOsShortcutError::InvalidStorageValue);
        }
        let mut command = false;
        let mut control = false;
        let mut option = false;
        let mut shift = false;
        let mut function = false;
        let mut key = None;
        for part in value.split('+') {
            match part {
                "command" => command = true,
                "control" => control = true,
                "option" => option = true,
                "shift" => shift = true,
                "fn" => function = true,
                value if key.is_none() => key = Some(value),
                _ => return Err(MacOsShortcutError::InvalidStorageValue),
            }
        }
        if let Some(code) = key.and_then(|key| key.strip_prefix("key-")) {
            let code = code
                .parse::<i64>()
                .map_err(|_| MacOsShortcutError::InvalidStorageValue)?;
            return Ok(Self {
                key: ShortcutKey::Physical(code),
                function,
                command,
                control,
                option,
                shift,
            });
        }
        let key = key.ok_or(MacOsShortcutError::InvalidStorageValue)?;
        Self::from_capture(key, command, control, option, shift)
    }

    pub fn storage_value(&self) -> String {
        if self == &Self::default() {
            return "right-command".to_owned();
        }
        if self == &Self::modifier(63) {
            return "fn".to_owned();
        }
        if let ShortcutKey::Modifier(code) = self.key {
            return format!("modifier-{code}");
        }
        let mut parts = Vec::with_capacity(6);
        if self.function {
            parts.push("fn".to_owned());
        }
        if self.command {
            parts.push("command".to_owned());
        }
        if self.control {
            parts.push("control".to_owned());
        }
        if self.option {
            parts.push("option".to_owned());
        }
        if self.shift {
            parts.push("shift".to_owned());
        }
        let ShortcutKey::Physical(code) = self.key else {
            return "right-command".to_owned();
        };
        parts.push(format!("key-{code}"));
        parts.join("+")
    }

    pub fn display_label(&self) -> String {
        if let ShortcutKey::Modifier(code) = self.key {
            return modifier_label(code).to_owned();
        }
        let mut label = String::new();
        if self.function {
            label.push_str("Fn ");
        }
        if self.control {
            label.push_str("⌃ ");
        }
        if self.option {
            label.push_str("⌥ ");
        }
        if self.shift {
            label.push_str("⇧ ");
        }
        if self.command {
            label.push_str("⌘ ");
        }
        let ShortcutKey::Physical(code) = self.key else {
            return label;
        };
        label.push_str(key_label(code));
        label
    }

    pub fn likely_system_conflict(&self) -> bool {
        let ShortcutKey::Physical(code) = self.key else {
            return false;
        };
        (self.command && matches!(code, 12 | 13 | 46 | 49 | 53))
            || (self.command && self.shift && matches!(code, 20 | 21 | 23))
    }

    fn validate(self) -> Result<Self, MacOsShortcutError> {
        if matches!(self.key, ShortcutKey::Physical(_))
            && !self.function
            && !self.command
            && !self.control
            && !self.option
        {
            return Err(MacOsShortcutError::MissingModifier);
        }
        if self.likely_system_conflict() {
            return Err(MacOsShortcutError::SystemReserved);
        }
        Ok(self)
    }

    fn matches_key_down(&self, event: &CGEvent) -> bool {
        let ShortcutKey::Physical(expected) = self.key else {
            return false;
        };
        key_code(event) == expected && self.matches_modifiers(event.get_flags())
    }

    fn matches_modifier_release(&self, code: i64) -> bool {
        self.key == ShortcutKey::Modifier(code)
    }

    fn matches_modifiers(&self, flags: CGEventFlags) -> bool {
        flags.contains(CGEventFlags::CGEventFlagSecondaryFn) == self.function
            && flags.contains(CGEventFlags::CGEventFlagCommand) == self.command
            && flags.contains(CGEventFlags::CGEventFlagControl) == self.control
            && flags.contains(CGEventFlags::CGEventFlagAlternate) == self.option
            && flags.contains(CGEventFlags::CGEventFlagShift) == self.shift
    }
}

#[derive(Clone)]
pub struct MacOsShortcutController {
    shortcuts: Arc<RwLock<Vec<MacOsShortcut>>>,
    capture: Arc<Mutex<Option<ShortcutCaptureSender>>>,
}

impl MacOsShortcutController {
    pub fn new(shortcuts: Vec<MacOsShortcut>) -> Self {
        Self {
            shortcuts: Arc::new(RwLock::new(if shortcuts.is_empty() {
                vec![MacOsShortcut::default()]
            } else {
                shortcuts
            })),
            capture: Arc::new(Mutex::new(None)),
        }
    }

    pub fn current(&self) -> Result<Vec<MacOsShortcut>, MacOsShortcutError> {
        self.shortcuts
            .read()
            .map(|shortcuts| shortcuts.clone())
            .map_err(|_| MacOsShortcutError::StateUnavailable)
    }

    pub fn replace(&self, shortcuts: Vec<MacOsShortcut>) -> Result<(), MacOsShortcutError> {
        if has_duplicates(&shortcuts) {
            return Err(MacOsShortcutError::Duplicate);
        }
        let mut current = self
            .shortcuts
            .write()
            .map_err(|_| MacOsShortcutError::StateUnavailable)?;
        *current = shortcuts;
        Ok(())
    }

    pub fn begin_capture(&self) -> Result<Receiver<ShortcutCaptureResult>, MacOsShortcutError> {
        let (sender, receiver) = channel();
        let mut capture = self
            .capture
            .lock()
            .map_err(|_| MacOsShortcutError::StateUnavailable)?;
        if capture.is_some() {
            return Err(MacOsShortcutError::CaptureActive);
        }
        *capture = Some(sender);
        Ok(receiver)
    }

    fn capturing(&self) -> bool {
        self.capture
            .lock()
            .map(|capture| capture.is_some())
            .unwrap_or(false)
    }

    fn reserves_fn(&self) -> bool {
        self.capturing()
            || self
                .current()
                .unwrap_or_default()
                .iter()
                .any(|shortcut| shortcut.matches_modifier_release(63))
    }

    fn finish_capture(&self, shortcut: MacOsShortcut) {
        if let Ok(mut capture) = self.capture.lock()
            && let Some(sender) = capture.take()
        {
            let _ = sender.send(shortcut.validate());
        }
    }

    fn cancel_capture(&self) {
        if let Ok(mut capture) = self.capture.lock()
            && let Some(sender) = capture.take()
        {
            let _ = sender.send(Err(MacOsShortcutError::CaptureCancelled));
        }
    }
}

/// Observes all configured global shortcuts and Escape while recording.
pub struct MacOsShortcutMonitor;

impl MacOsShortcutMonitor {
    pub fn start(
        is_recording: Arc<dyn Fn() -> bool + Send + Sync>,
        controller: MacOsShortcutController,
        on_action: impl Fn(DictationShortcutAction) + Send + 'static,
        on_permission_required: impl Fn() + Send + 'static,
    ) {
        event_tap::start(is_recording, controller, on_action, on_permission_required);
    }
}

fn has_duplicates(shortcuts: &[MacOsShortcut]) -> bool {
    shortcuts
        .iter()
        .enumerate()
        .any(|(index, shortcut)| shortcuts[..index].contains(shortcut))
}

fn cancel_shortcut(key_code: i64, recording_active: bool) -> bool {
    key_code == ESCAPE_KEY_CODE && recording_active
}

fn key_code(event: &CGEvent) -> i64 {
    event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE)
}

#[cfg(test)]
mod tests;
