use std::collections::HashSet;

use core_graphics::{event::CGEventFlags, event_source::CGEventSourceStateID};

use super::{MacOsShortcut, ShortcutKey};

const MAX_KEY_CODE: i64 = 127;

#[derive(Default)]
pub(super) struct UntrustedShortcutDetector {
    previously_down: HashSet<i64>,
    used_modifiers: HashSet<i64>,
}

impl UntrustedShortcutDetector {
    pub(super) fn observe_system(&mut self, shortcuts: &[MacOsShortcut]) -> bool {
        // SAFETY: these CoreGraphics functions only read the current login
        // session's global event-state table and accept every key code we pass.
        let flags = CGEventFlags::from_bits_truncate(unsafe {
            CGEventSourceFlagsState(CGEventSourceStateID::CombinedSessionState)
        });
        self.observe(shortcuts, flags, |code| unsafe {
            CGEventSourceKeyState(CGEventSourceStateID::CombinedSessionState, code as u16)
        })
    }

    pub(super) fn observe(
        &mut self,
        shortcuts: &[MacOsShortcut],
        flags: CGEventFlags,
        mut key_down: impl FnMut(i64) -> bool,
    ) -> bool {
        let mut current = shortcuts
            .iter()
            .map(|shortcut| match shortcut.key {
                ShortcutKey::Modifier(code) | ShortcutKey::Physical(code) => code,
            })
            .filter(|code| key_down(*code))
            .collect::<HashSet<_>>();
        let configured_modifier_down = shortcuts.iter().any(|shortcut| {
            matches!(shortcut.key, ShortcutKey::Modifier(code) if current.contains(&code))
        });
        if configured_modifier_down {
            current.extend((0..=MAX_KEY_CODE).filter(|code| key_down(*code)));
        }
        let mut triggered = false;

        for shortcut in shortcuts {
            match shortcut.key {
                ShortcutKey::Modifier(code) => {
                    if current.contains(&code) {
                        if current.iter().any(|active| *active != code) {
                            self.used_modifiers.insert(code);
                        }
                    } else if self.previously_down.contains(&code) {
                        triggered |= !self.used_modifiers.remove(&code);
                    }
                }
                ShortcutKey::Physical(code) => {
                    triggered |= current.contains(&code)
                        && !self.previously_down.contains(&code)
                        && shortcut.matches_modifiers(flags);
                }
            }
        }
        self.previously_down = current;
        triggered
    }
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventSourceKeyState(state_id: CGEventSourceStateID, key: u16) -> bool;
    fn CGEventSourceFlagsState(state_id: CGEventSourceStateID) -> u64;
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::macos_shortcut_monitor::MacOsShortcut;

    fn observe(
        detector: &mut UntrustedShortcutDetector,
        shortcut: &MacOsShortcut,
        down: &[i64],
        flags: CGEventFlags,
    ) -> bool {
        let down = down.iter().copied().collect::<HashSet<_>>();
        detector.observe(std::slice::from_ref(shortcut), flags, |code| {
            down.contains(&code)
        })
    }

    #[test]
    fn standalone_modifier_release_requests_permission_once() {
        let shortcut = MacOsShortcut::modifier(54);
        let mut detector = UntrustedShortcutDetector::default();

        assert!(!observe(
            &mut detector,
            &shortcut,
            &[54],
            CGEventFlags::CGEventFlagCommand
        ));
        assert!(observe(
            &mut detector,
            &shortcut,
            &[],
            CGEventFlags::empty()
        ));
        assert!(!observe(
            &mut detector,
            &shortcut,
            &[],
            CGEventFlags::empty()
        ));
    }

    #[test]
    fn modifier_used_in_a_chord_does_not_request_permission() {
        let shortcut = MacOsShortcut::modifier(54);
        let mut detector = UntrustedShortcutDetector::default();

        assert!(!observe(
            &mut detector,
            &shortcut,
            &[54],
            CGEventFlags::CGEventFlagCommand
        ));
        assert!(!observe(
            &mut detector,
            &shortcut,
            &[54, 8],
            CGEventFlags::CGEventFlagCommand
        ));
        assert!(!observe(
            &mut detector,
            &shortcut,
            &[],
            CGEventFlags::empty()
        ));
    }

    #[test]
    fn configured_chord_requests_permission_on_its_press_edge() {
        let shortcut = MacOsShortcut::from_capture("K", true, false, false, false)
            .unwrap_or_else(|error| panic!("test shortcut must be valid: {error}"));
        let mut detector = UntrustedShortcutDetector::default();

        assert!(observe(
            &mut detector,
            &shortcut,
            &[40, 54],
            CGEventFlags::CGEventFlagCommand
        ));
        assert!(!observe(
            &mut detector,
            &shortcut,
            &[40, 54],
            CGEventFlags::CGEventFlagCommand
        ));
    }

    #[test]
    fn idle_poll_queries_only_configured_keys() {
        let shortcut = MacOsShortcut::from_capture("K", true, false, false, false)
            .unwrap_or_else(|error| panic!("test shortcut must be valid: {error}"));
        let mut detector = UntrustedShortcutDetector::default();
        let mut queried = Vec::new();

        assert!(!detector.observe(
            std::slice::from_ref(&shortcut),
            CGEventFlags::empty(),
            |code| {
                queried.push(code);
                false
            }
        ));
        assert_eq!(vec![40], queried);
    }
}
