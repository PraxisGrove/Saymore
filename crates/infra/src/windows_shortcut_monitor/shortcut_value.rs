use windows::Win32::UI::Input::KeyboardAndMouse::{MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN};

use super::WindowsShortcutError;

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
    pub(super) modifiers: u32,
    pub(super) virtual_key: u32,
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

    pub(crate) fn new(modifiers: u32, virtual_key: u32) -> Result<Self, WindowsShortcutError> {
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

    pub(crate) fn is_right_alt(self) -> bool {
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

pub(crate) fn key_label(virtual_key: u32) -> Option<&'static str> {
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

pub(crate) fn is_modifier_key(key: u32) -> bool {
    matches!(key, VK_SHIFT | VK_CONTROL | VK_MENU | VK_LWIN | VK_RWIN)
}
