use std::{
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};

use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN,
};

use crate::windows_shortcut_monitor::{
    WindowsShortcut, WindowsShortcutError, is_modifier_key, key_label,
};

const POLL_INTERVAL: Duration = Duration::from_millis(10);
const ESCAPE_VK: u32 = 0x1b;
const VK_BACK: u32 = 0x08;
const VK_SHIFT: u32 = 0x10;
const VK_CONTROL: u32 = 0x11;
const VK_MENU: u32 = 0x12;
const VK_LWIN: u32 = 0x5b;
const VK_RWIN: u32 = 0x5c;
const VK_RMENU: u32 = 0xa5;

pub(super) fn capture_shortcut(
    capture_active: &AtomicBool,
    runtime_closed: &AtomicBool,
) -> Result<WindowsShortcut, WindowsShortcutError> {
    capture_active
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| WindowsShortcutError::CaptureActive)?;
    let _guard = CaptureGuard(capture_active);
    wait_for_supported_keys_to_release(runtime_closed)?;
    loop {
        ensure_open(runtime_closed)?;
        if key_is_down(VK_RMENU) {
            wait_for_key_release(VK_RMENU, runtime_closed)?;
            return Ok(WindowsShortcut::default());
        }
        if let Some(result) = capture_observation(
            key_is_down(ESCAPE_VK),
            pressed_supported_key(),
            active_modifiers(),
        ) {
            wait_for_supported_keys_to_release(runtime_closed)?;
            return result;
        }
        thread::sleep(POLL_INTERVAL);
    }
}

struct CaptureGuard<'a>(&'a AtomicBool);

impl Drop for CaptureGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

fn wait_for_supported_keys_to_release(closed: &AtomicBool) -> Result<(), WindowsShortcutError> {
    while pressed_supported_key().is_some() || key_is_down(ESCAPE_VK) {
        ensure_open(closed)?;
        thread::sleep(POLL_INTERVAL);
    }
    Ok(())
}

fn wait_for_key_release(key: u32, closed: &AtomicBool) -> Result<(), WindowsShortcutError> {
    while key_is_down(key) {
        ensure_open(closed)?;
        thread::sleep(POLL_INTERVAL);
    }
    Ok(())
}

fn ensure_open(closed: &AtomicBool) -> Result<(), WindowsShortcutError> {
    if closed.load(Ordering::Acquire) {
        Err(WindowsShortcutError::RuntimeClosed)
    } else {
        Ok(())
    }
}

fn active_modifiers() -> u32 {
    let mut modifiers = 0;
    if key_is_down(VK_CONTROL) {
        modifiers |= MOD_CONTROL.0;
    }
    if key_is_down(VK_MENU) {
        modifiers |= MOD_ALT.0;
    }
    if key_is_down(VK_SHIFT) {
        modifiers |= MOD_SHIFT.0;
    }
    if key_is_down(VK_LWIN) || key_is_down(VK_RWIN) {
        modifiers |= MOD_WIN.0;
    }
    modifiers
}

pub(super) fn capture_observation(
    escape_pressed: bool,
    key: Option<u32>,
    modifiers: u32,
) -> Option<Result<WindowsShortcut, WindowsShortcutError>> {
    if escape_pressed {
        Some(Err(WindowsShortcutError::CaptureCancelled))
    } else {
        key.map(|key| WindowsShortcut::new(modifiers, key))
    }
}

fn pressed_supported_key() -> Option<u32> {
    (VK_BACK..=0x87)
        .filter(|key| !is_modifier_key(*key) && *key != ESCAPE_VK)
        .find(|key| key_label(*key).is_some() && key_is_down(*key))
}

fn key_is_down(key: u32) -> bool {
    i32::from(unsafe { GetAsyncKeyState(key as i32) }) & 0x8000 != 0
}
