use accessibility_sys::AXIsProcessTrusted;
use core_foundation::runloop::{CFRunLoop, kCFRunLoopCommonModes};
use core_graphics::event::{
    CGEvent, CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventType, CallbackResult, EventField,
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Sender, channel},
    },
    thread,
    time::Duration,
};

const PERMISSION_RETRY_INTERVAL: Duration = Duration::from_secs(1);
const RIGHT_COMMAND_KEY_CODE: i64 = 54;
const ESCAPE_KEY_CODE: i64 = 53;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DictationShortcutAction {
    Toggle,
    Cancel,
}

/// Observes complete Right Command clicks without treating the press and release
/// as separate recording commands.
pub struct MacOsShortcutMonitor;

impl MacOsShortcutMonitor {
    pub fn start(
        recording_active: Arc<AtomicBool>,
        on_action: impl Fn(DictationShortcutAction) + Send + 'static,
    ) {
        let (sender, receiver) = channel();
        thread::spawn(move || {
            for action in receiver {
                on_action(action);
            }
        });

        thread::spawn(move || {
            loop {
                // SAFETY: AXIsProcessTrusted has no preconditions and only reads TCC state.
                if unsafe { AXIsProcessTrusted() }
                    && run_event_tap(sender.clone(), Arc::clone(&recording_active)).is_ok()
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
    recording_active: Arc<AtomicBool>,
) -> Result<(), ()> {
    let is_right_command_down = AtomicBool::new(false);
    let event_tap = CGEventTap::new(
        CGEventTapLocation::HID,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::Default,
        vec![CGEventType::FlagsChanged, CGEventType::KeyDown],
        move |_proxy, event_type, event: &CGEvent| match event_type {
            CGEventType::FlagsChanged => {
                if update_right_command_state(&is_right_command_down, event) {
                    let _ = sender.send(DictationShortcutAction::Toggle);
                }
                CallbackResult::Keep
            }
            CGEventType::KeyDown
                if cancel_shortcut(key_code(event), recording_active.load(Ordering::Relaxed)) =>
            {
                let _ = sender.send(DictationShortcutAction::Cancel);
                CallbackResult::Drop
            }
            _ => CallbackResult::Keep,
        },
    )?;
    let source = event_tap.mach_port().create_runloop_source(0)?;
    CFRunLoop::get_current().add_source(&source, unsafe { kCFRunLoopCommonModes });
    event_tap.enable();
    CFRunLoop::run_current();
    Ok(())
}

fn update_right_command_state(state: &AtomicBool, event: &CGEvent) -> bool {
    if key_code(event) != RIGHT_COMMAND_KEY_CODE {
        return false;
    }

    let is_down = event.get_flags().contains(CGEventFlags::CGEventFlagCommand);
    let was_down = state.swap(is_down, Ordering::Relaxed);
    shortcut_clicked(key_code(event), is_down, was_down)
}

fn shortcut_clicked(key_code: i64, is_down: bool, was_down: bool) -> bool {
    key_code == RIGHT_COMMAND_KEY_CODE && was_down && !is_down
}

fn cancel_shortcut(key_code: i64, recording_active: bool) -> bool {
    key_code == ESCAPE_KEY_CODE && recording_active
}

fn key_code(event: &CGEvent) -> i64 {
    event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_one_click_when_right_command_is_released() {
        assert_eq!(
            [false, false, true, false],
            [
                shortcut_clicked(54, true, false),
                shortcut_clicked(54, true, true),
                shortcut_clicked(54, false, true),
                shortcut_clicked(55, false, true),
            ]
        );
    }

    #[test]
    fn consumes_escape_only_while_recording() {
        assert!(!cancel_shortcut(ESCAPE_KEY_CODE, false));
        assert!(cancel_shortcut(ESCAPE_KEY_CODE, true));
        assert!(!cancel_shortcut(RIGHT_COMMAND_KEY_CODE, true));
    }
}
