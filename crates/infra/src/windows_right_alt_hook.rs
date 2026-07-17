use std::sync::{Mutex, OnceLock, mpsc::SyncSender};

use windows::Win32::{
    Foundation::{LPARAM, LRESULT, WPARAM},
    UI::Input::KeyboardAndMouse::VK_MENU,
    UI::WindowsAndMessaging::{
        CallNextHookEx, HHOOK, KBDLLHOOKSTRUCT, LLKHF_EXTENDED, SetWindowsHookExW,
        UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_KEYUP, WM_SYSKEYUP,
    },
};

use crate::windows_shortcut_monitor::WindowsShortcutError;

const RIGHT_ALT_VK: u32 = 0xa5;
static EVENT_SENDER: OnceLock<Mutex<Option<SyncSender<()>>>> = OnceLock::new();

pub(super) struct WindowsRightAltHook {
    hook: HHOOK,
}

impl WindowsRightAltHook {
    pub(super) fn install(sender: SyncSender<()>) -> Result<Self, WindowsShortcutError> {
        let slot = EVENT_SENDER.get_or_init(|| Mutex::new(None));
        let mut current = slot
            .lock()
            .map_err(|_| WindowsShortcutError::StateUnavailable)?;
        if current.is_some() {
            return Err(WindowsShortcutError::StateUnavailable);
        }
        *current = Some(sender);
        match unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), None, 0) } {
            Ok(hook) => Ok(Self { hook }),
            Err(error) => {
                current.take();
                Err(WindowsShortcutError::RegistrationConflict {
                    shortcut: "Right Alt".to_owned(),
                    reason: error.to_string(),
                })
            }
        }
    }
}

impl Drop for WindowsRightAltHook {
    fn drop(&mut self) {
        if let Err(error) = unsafe { UnhookWindowsHookEx(self.hook) } {
            tracing::warn!(event = "shortcut.right_alt_unhook_failed", reason = %error);
        }
        if let Some(slot) = EVENT_SENDER.get()
            && let Ok(mut sender) = slot.lock()
        {
            sender.take();
        }
    }
}

unsafe extern "system" fn keyboard_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && matches!(wparam.0 as u32, WM_KEYUP | WM_SYSKEYUP) {
        let event = unsafe { (lparam.0 as *const KBDLLHOOKSTRUCT).as_ref() };
        if let Some(event) = event
            && is_right_alt(event)
            && let Some(slot) = EVENT_SENDER.get()
            && let Ok(sender) = slot.lock()
            && let Some(sender) = sender.as_ref()
        {
            let _ = sender.try_send(());
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

fn is_right_alt(event: &KBDLLHOOKSTRUCT) -> bool {
    event.vkCode == RIGHT_ALT_VK
        || (event.vkCode == VK_MENU.0 as u32 && event.flags.contains(LLKHF_EXTENDED))
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    #[test]
    fn hook_can_be_reinstalled_after_drop() {
        let (events, _received_events) = mpsc::sync_channel(1);
        let first = WindowsRightAltHook::install(events.clone());
        assert!(first.is_ok());
        drop(first);

        let second = WindowsRightAltHook::install(events);
        assert!(second.is_ok());
        drop(second);
    }

    #[test]
    fn recognizes_native_and_extended_right_alt_events() {
        let native = KBDLLHOOKSTRUCT {
            vkCode: RIGHT_ALT_VK,
            ..Default::default()
        };
        assert!(is_right_alt(&native));

        let extended = KBDLLHOOKSTRUCT {
            vkCode: VK_MENU.0 as u32,
            flags: LLKHF_EXTENDED,
            ..Default::default()
        };
        assert!(is_right_alt(&extended));

        let left_alt = KBDLLHOOKSTRUCT {
            vkCode: VK_MENU.0 as u32,
            ..Default::default()
        };
        assert!(!is_right_alt(&left_alt));
    }
}
