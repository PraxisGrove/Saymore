use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, Ordering},
    mpsc::SyncSender,
};

use windows::Win32::{
    Foundation::{LPARAM, LRESULT, WPARAM},
    UI::Input::KeyboardAndMouse::VK_MENU,
    UI::WindowsAndMessaging::{
        CallNextHookEx, HHOOK, KBDLLHOOKSTRUCT, LLKHF_EXTENDED, SetWindowsHookExW,
        UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
    },
};

use crate::windows_shortcut_monitor::WindowsShortcutError;

const RIGHT_ALT_VK: u32 = 0xa5;
static HOOK_TARGET: OnceLock<Mutex<Option<HookTarget>>> = OnceLock::new();

struct HookTarget {
    sender: SyncSender<()>,
    enabled: Arc<AtomicBool>,
    press_active: bool,
}

pub(super) struct WindowsRightAltHook {
    hook: HHOOK,
}

impl WindowsRightAltHook {
    pub(super) fn install(
        sender: SyncSender<()>,
        enabled: Arc<AtomicBool>,
    ) -> Result<Self, WindowsShortcutError> {
        let slot = HOOK_TARGET.get_or_init(|| Mutex::new(None));
        let mut current = slot
            .lock()
            .map_err(|_| WindowsShortcutError::StateUnavailable)?;
        if current.is_some() {
            return Err(WindowsShortcutError::StateUnavailable);
        }
        *current = Some(HookTarget {
            sender,
            enabled,
            press_active: false,
        });
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
        if let Some(slot) = HOOK_TARGET.get()
            && let Ok(mut target) = slot.lock()
        {
            target.take();
        }
    }
}

unsafe extern "system" fn keyboard_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let event = unsafe { (lparam.0 as *const KBDLLHOOKSTRUCT).as_ref() };
    if let Some(slot) = HOOK_TARGET.get()
        && let Ok(mut target) = slot.lock()
        && let Some(target) = target.as_mut()
    {
        let enabled = target.enabled.load(Ordering::Acquire);
        let action = classify_hook_event(
            code,
            wparam.0 as u32,
            event,
            enabled,
            &mut target.press_active,
        );
        if action.notify {
            let _ = target.sender.try_send(());
        }
        if action.suppress {
            return LRESULT(1);
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HookAction {
    notify: bool,
    suppress: bool,
}

fn classify_hook_event(
    code: i32,
    message: u32,
    event: Option<&KBDLLHOOKSTRUCT>,
    enabled: bool,
    press_active: &mut bool,
) -> HookAction {
    if code < 0 || !event.is_some_and(is_right_alt) {
        return HookAction::forward();
    }
    if matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN) {
        if !enabled {
            return HookAction::forward();
        }
        *press_active = true;
        return HookAction {
            notify: false,
            suppress: true,
        };
    }
    if matches!(message, WM_KEYUP | WM_SYSKEYUP) && *press_active {
        *press_active = false;
        return HookAction {
            notify: true,
            suppress: true,
        };
    }
    HookAction::forward()
}

impl HookAction {
    const fn forward() -> Self {
        Self {
            notify: false,
            suppress: false,
        }
    }
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
        let enabled = Arc::new(AtomicBool::new(true));
        let first = WindowsRightAltHook::install(events.clone(), Arc::clone(&enabled));
        assert!(first.is_ok());
        drop(first);

        let second = WindowsRightAltHook::install(events, enabled);
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

    #[test]
    fn bound_right_alt_press_is_consumed_without_reaching_the_foreground_app() {
        let right_alt = KBDLLHOOKSTRUCT {
            vkCode: RIGHT_ALT_VK,
            ..Default::default()
        };
        let mut press_active = false;

        assert_eq!(
            HookAction {
                notify: false,
                suppress: true,
            },
            classify_hook_event(0, WM_SYSKEYDOWN, Some(&right_alt), true, &mut press_active)
        );
        assert_eq!(
            HookAction {
                notify: true,
                suppress: true,
            },
            classify_hook_event(0, WM_SYSKEYUP, Some(&right_alt), false, &mut press_active)
        );
    }

    #[test]
    fn unrelated_or_disabled_keyboard_events_continue_to_the_foreground_app() {
        let left_alt = KBDLLHOOKSTRUCT {
            vkCode: VK_MENU.0 as u32,
            ..Default::default()
        };
        let right_alt = KBDLLHOOKSTRUCT {
            vkCode: RIGHT_ALT_VK,
            ..Default::default()
        };
        let mut press_active = false;

        assert_eq!(
            HookAction {
                notify: false,
                suppress: false,
            },
            classify_hook_event(0, WM_KEYDOWN, Some(&left_alt), true, &mut press_active)
        );
        assert_eq!(
            HookAction::forward(),
            classify_hook_event(0, WM_SYSKEYDOWN, Some(&right_alt), false, &mut press_active,)
        );
        assert_eq!(
            HookAction::forward(),
            classify_hook_event(0, WM_SYSKEYUP, Some(&right_alt), true, &mut press_active)
        );
    }
}
