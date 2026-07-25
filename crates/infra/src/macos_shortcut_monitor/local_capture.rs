use std::{collections::HashSet, ptr::NonNull, sync::Mutex};

use block2::RcBlock;
use objc2::{rc::Retained, runtime::AnyObject};
use objc2_app_kit::{NSEvent, NSEventMask, NSEventType};

use super::*;

pub(super) struct LocalShortcutCaptureMonitor {
    monitor: Option<Retained<AnyObject>>,
}

impl LocalShortcutCaptureMonitor {
    pub(super) fn install(controller: MacOsShortcutController) -> Self {
        let state = Mutex::new(LocalCaptureState::default());
        let handler = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
            let event_pointer = event.as_ptr();
            if !controller.capturing() {
                if let Ok(mut state) = state.lock() {
                    state.reset();
                }
                return event_pointer;
            }

            // SAFETY: AppKit provides a valid NSEvent pointer for the duration of the block.
            let event = unsafe { event.as_ref() };
            let event_type = event.r#type();
            let code = i64::from(event.keyCode());
            let observation = match event_type {
                NSEventType::KeyDown => state.lock().ok().and_then(|mut state| {
                    state.observe_key_down(code, cg_flags(event), event.isARepeat())
                }),
                NSEventType::FlagsChanged => state
                    .lock()
                    .ok()
                    .and_then(|mut state| state.observe_modifier(code, cg_flags(event))),
                _ => return event_pointer,
            };
            if let Some(observation) = observation {
                if let Ok(mut state) = state.lock() {
                    state.reset();
                }
                observation.finish(&controller);
            }
            if event_type == NSEventType::KeyDown || code == 63 {
                std::ptr::null_mut()
            } else {
                event_pointer
            }
        });
        let mask = NSEventMask::KeyDown | NSEventMask::FlagsChanged | NSEventMask::LeftMouseDown;
        // SAFETY: the block returns either the original live event or null to consume it.
        let monitor =
            unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &handler) };
        if monitor.is_none() {
            tracing::warn!(event = "shortcut.local_capture_monitor_failed");
        }
        Self { monitor }
    }
}

impl Drop for LocalShortcutCaptureMonitor {
    fn drop(&mut self) {
        if let Some(monitor) = self.monitor.take() {
            // SAFETY: this is the monitor token returned by AppKit and is removed once.
            unsafe { NSEvent::removeMonitor(&monitor) };
        }
    }
}

#[derive(Default)]
struct LocalCaptureState {
    modifiers_down: HashSet<i64>,
}

impl LocalCaptureState {
    fn observe_key_down(
        &mut self,
        code: i64,
        flags: CGEventFlags,
        repeated: bool,
    ) -> Option<CaptureObservation> {
        if repeated {
            None
        } else if code == ESCAPE_KEY_CODE {
            Some(CaptureObservation::Cancel)
        } else {
            Some(CaptureObservation::Shortcut(MacOsShortcut::physical(
                code, flags,
            )))
        }
    }

    fn observe_modifier(&mut self, code: i64, flags: CGEventFlags) -> Option<CaptureObservation> {
        if modifier_is_down(code, flags) {
            self.modifiers_down.insert(code);
            return None;
        }
        self.modifiers_down
            .remove(&code)
            .then(|| CaptureObservation::Shortcut(MacOsShortcut::modifier(code)))
    }

    fn reset(&mut self) {
        self.modifiers_down.clear();
    }
}

fn cg_flags(event: &NSEvent) -> CGEventFlags {
    CGEventFlags::from_bits_truncate(event.modifierFlags().bits() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn right_option_event_keeps_its_physical_side() {
        let mut state = LocalCaptureState::default();

        assert_eq!(
            None,
            state.observe_modifier(61, CGEventFlags::CGEventFlagAlternate)
        );
        let observed = state.observe_modifier(61, CGEventFlags::empty());

        assert_eq!(
            Some(CaptureObservation::Shortcut(MacOsShortcut::modifier(61))),
            observed
        );
    }
}
