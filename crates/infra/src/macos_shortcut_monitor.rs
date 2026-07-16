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

const PERMISSION_RETRY_INTERVAL: Duration = Duration::from_secs(1);
const RIGHT_COMMAND_KEY_CODE: i64 = 54;
const ESCAPE_KEY_CODE: i64 = 53;
type ShortcutCaptureResult = Result<MacOsShortcut, MacOsShortcutError>;
type ShortcutCaptureSender = Sender<ShortcutCaptureResult>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DictationShortcutAction {
    Toggle,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MacOsShortcutError {
    #[error("this shortcut is already configured")]
    Duplicate,
    #[error("this shortcut requires Command, Control, Option, or Fn")]
    MissingModifier,
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
    ) {
        let (sender, receiver) = channel();
        thread::spawn(move || {
            for action in receiver {
                on_action(action);
            }
        });
        thread::spawn(move || {
            #[cfg(debug_assertions)]
            eprintln!("saymore_fn_trace phase=monitor-thread-started");
            #[cfg(debug_assertions)]
            let mut trust_logged = false;
            loop {
                // SAFETY: AXIsProcessTrusted has no preconditions and only reads TCC state.
                let trusted = unsafe { AXIsProcessTrusted() };
                #[cfg(debug_assertions)]
                if !trust_logged {
                    eprintln!("saymore_fn_trace phase=accessibility-check trusted={trusted}");
                    trust_logged = true;
                }
                if trusted
                    && run_event_tap(
                        sender.clone(),
                        Arc::clone(&is_recording),
                        controller.clone(),
                    )
                    .is_ok()
                {
                    return;
                }
                thread::sleep(PERMISSION_RETRY_INTERVAL);
            }
        });
    }
}

fn run_event_tap(
    sender: Sender<DictationShortcutAction>,
    is_recording: Arc<dyn Fn() -> bool + Send + Sync>,
    controller: MacOsShortcutController,
) -> Result<(), ()> {
    let modifier_state = Mutex::new(ModifierState::default());
    let event_tap = CGEventTap::new(
        CGEventTapLocation::HID,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::Default,
        vec![
            CGEventType::FlagsChanged,
            CGEventType::KeyDown,
            CGEventType::KeyUp,
        ],
        move |_proxy, event_type, event: &CGEvent| {
            #[cfg(debug_assertions)]
            if key_code(event) == 63
                || event
                    .get_flags()
                    .contains(CGEventFlags::CGEventFlagSecondaryFn)
            {
                eprintln!(
                    "saymore_fn_trace phase=received event_type={event_type:?} key_code={} flags={:#x}",
                    key_code(event),
                    event.get_flags().bits()
                );
            }
            match event_type {
                CGEventType::FlagsChanged => {
                    handle_modifier_event(event, &modifier_state, &controller, &sender)
                }
                CGEventType::KeyDown | CGEventType::KeyUp
                    if key_code(event) == 63 && controller.reserves_fn() =>
                {
                    CallbackResult::Drop
                }
                CGEventType::KeyDown
                    if cancel_shortcut(key_code(event), is_recording())
                        && !controller.capturing() =>
                {
                    mark_active_modifiers_used(&modifier_state);
                    let _ = sender.send(DictationShortcutAction::Cancel);
                    CallbackResult::Drop
                }
                CGEventType::KeyDown if controller.capturing() => {
                    mark_active_modifiers_used(&modifier_state);
                    if !is_repeat(event) {
                        if key_code(event) == ESCAPE_KEY_CODE {
                            controller.cancel_capture();
                        } else {
                            controller.finish_capture(MacOsShortcut::physical(
                                key_code(event),
                                event.get_flags(),
                            ));
                        }
                    }
                    CallbackResult::Drop
                }
                CGEventType::KeyDown => {
                    mark_active_modifiers_used(&modifier_state);
                    let matches = controller
                        .current()
                        .unwrap_or_default()
                        .iter()
                        .any(|shortcut| shortcut.matches_key_down(event));
                    if matches {
                        if !is_repeat(event) {
                            let _ = sender.send(DictationShortcutAction::Toggle);
                        }
                        CallbackResult::Drop
                    } else {
                        CallbackResult::Keep
                    }
                }
                _ => CallbackResult::Keep,
            }
        },
    );
    #[cfg(debug_assertions)]
    if event_tap.is_err() {
        eprintln!("saymore_fn_trace phase=event-tap-create result=failed");
    }
    let event_tap = event_tap?;
    let source = event_tap.mach_port().create_runloop_source(0)?;
    CFRunLoop::get_current().add_source(&source, unsafe { kCFRunLoopCommonModes });
    event_tap.enable();
    #[cfg(debug_assertions)]
    eprintln!("saymore_fn_trace phase=event-tap-enabled");
    CFRunLoop::run_current();
    Ok(())
}

#[derive(Default)]
struct ModifierState {
    down: HashSet<i64>,
    used_in_chord: HashSet<i64>,
    suppressed: HashSet<i64>,
}

fn handle_modifier_event(
    event: &CGEvent,
    modifier_state: &Mutex<ModifierState>,
    controller: &MacOsShortcutController,
    sender: &Sender<DictationShortcutAction>,
) -> CallbackResult {
    let code = key_code(event);
    if !is_modifier_key(code) {
        return CallbackResult::Keep;
    }
    let Ok(mut state) = modifier_state.lock() else {
        return CallbackResult::Keep;
    };
    if modifier_is_down(code, event.get_flags()) {
        state.down.insert(code);
        state.used_in_chord.remove(&code);
        let suppress = code == 63 && controller.reserves_fn();
        if suppress {
            #[cfg(debug_assertions)]
            eprintln!("saymore_fn_trace phase=modifier-down result=drop");
            state.suppressed.insert(code);
            return CallbackResult::Drop;
        }
        state.suppressed.remove(&code);
        return CallbackResult::Keep;
    }
    let was_down = state.down.remove(&code);
    let used_in_chord = state.used_in_chord.remove(&code);
    let suppressed = state.suppressed.remove(&code);
    drop(state);
    if !was_down {
        return CallbackResult::Keep;
    }
    if used_in_chord {
        #[cfg(debug_assertions)]
        if code == 63 {
            eprintln!(
                "saymore_fn_trace phase=modifier-up chord=true result={}",
                if suppressed { "drop" } else { "keep" }
            );
        }
        return if suppressed {
            CallbackResult::Drop
        } else {
            CallbackResult::Keep
        };
    }
    if controller.capturing() {
        controller.finish_capture(MacOsShortcut::modifier(code));
        return CallbackResult::Drop;
    }
    let matches_shortcut = controller
        .current()
        .unwrap_or_default()
        .iter()
        .any(|shortcut| shortcut.matches_modifier_release(code));
    if matches_shortcut {
        #[cfg(debug_assertions)]
        if code == 63 {
            eprintln!("saymore_fn_trace phase=modifier-up toggle=true result=drop");
        }
        let _ = sender.send(DictationShortcutAction::Toggle);
        return CallbackResult::Drop;
    }
    CallbackResult::Keep
}

fn mark_active_modifiers_used(modifier_state: &Mutex<ModifierState>) {
    if let Ok(mut state) = modifier_state.lock() {
        let down = state.down.iter().copied().collect::<Vec<_>>();
        state.used_in_chord.extend(down);
    }
}

fn has_duplicates(shortcuts: &[MacOsShortcut]) -> bool {
    shortcuts
        .iter()
        .enumerate()
        .any(|(index, shortcut)| shortcuts[..index].contains(shortcut))
}

fn is_repeat(event: &CGEvent) -> bool {
    event.get_integer_value_field(EventField::KEYBOARD_EVENT_AUTOREPEAT) != 0
}

fn cancel_shortcut(key_code: i64, recording_active: bool) -> bool {
    key_code == ESCAPE_KEY_CODE && recording_active
}

fn key_code(event: &CGEvent) -> i64 {
    event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE)
}

fn is_modifier_key(code: i64) -> bool {
    matches!(code, 54..=63)
}

fn modifier_is_down(code: i64, flags: CGEventFlags) -> bool {
    let flag = match code {
        54 | 55 => CGEventFlags::CGEventFlagCommand,
        56 | 60 => CGEventFlags::CGEventFlagShift,
        58 | 61 => CGEventFlags::CGEventFlagAlternate,
        59 | 62 => CGEventFlags::CGEventFlagControl,
        63 => CGEventFlags::CGEventFlagSecondaryFn,
        57 => CGEventFlags::CGEventFlagAlphaShift,
        _ => return false,
    };
    flags.contains(flag)
}

fn modifier_label(code: i64) -> &'static str {
    match code {
        54 => "Right Command",
        55 => "Left Command",
        56 => "Left Shift",
        57 => "Caps Lock",
        58 => "Left Option",
        59 => "Left Control",
        60 => "Right Shift",
        61 => "Right Option",
        62 => "Right Control",
        63 => "Fn",
        _ => "Modifier",
    }
}

fn key_label(code: i64) -> &'static str {
    match code {
        0 => "A",
        1 => "S",
        2 => "D",
        3 => "F",
        4 => "H",
        5 => "G",
        6 => "Z",
        7 => "X",
        8 => "C",
        9 => "V",
        11 => "B",
        12 => "Q",
        13 => "W",
        14 => "E",
        15 => "R",
        16 => "Y",
        17 => "T",
        18 => "1",
        19 => "2",
        20 => "3",
        21 => "4",
        22 => "6",
        23 => "5",
        24 => "=",
        25 => "9",
        26 => "7",
        27 => "-",
        28 => "8",
        29 => "0",
        30 => "]",
        31 => "O",
        32 => "U",
        33 => "[",
        34 => "I",
        35 => "P",
        36 => "Return",
        37 => "L",
        38 => "J",
        39 => "'",
        40 => "K",
        41 => ";",
        42 => "\\",
        43 => ",",
        44 => "/",
        45 => "N",
        46 => "M",
        47 => ".",
        48 => "Tab",
        49 => "Space",
        50 => "`",
        51 => "Delete",
        53 => "Escape",
        64 => "F17",
        65 => "Keypad .",
        67 => "Keypad *",
        69 => "Keypad +",
        71 => "Clear",
        75 => "Keypad /",
        76 => "Keypad Enter",
        78 => "Keypad -",
        79 => "F18",
        80 => "F19",
        81 => "Keypad =",
        82 => "Keypad 0",
        83 => "Keypad 1",
        84 => "Keypad 2",
        85 => "Keypad 3",
        86 => "Keypad 4",
        87 => "Keypad 5",
        88 => "Keypad 6",
        89 => "Keypad 7",
        91 => "Keypad 8",
        92 => "Keypad 9",
        96 => "F5",
        97 => "F6",
        98 => "F7",
        99 => "F3",
        100 => "F8",
        101 => "F9",
        103 => "F11",
        105 => "F13",
        106 => "F16",
        107 => "F14",
        109 => "F10",
        111 => "F12",
        113 => "F15",
        114 => "Help",
        115 => "Home",
        116 => "Page Up",
        117 => "Forward Delete",
        118 => "F4",
        119 => "End",
        120 => "F2",
        121 => "Page Down",
        122 => "F1",
        123 => "Left",
        124 => "Right",
        125 => "Down",
        126 => "Up",
        _ => "Unknown Key",
    }
}

fn key_code_for_character(value: char) -> Option<i64> {
    (0..=126).find(|code| key_label(*code).starts_with(value))
}

#[cfg(test)]
mod tests;
