use std::collections::HashSet;

use core_graphics::{event::CGEventFlags, event_source::CGEventSourceStateID};

use super::{CaptureObservation, ESCAPE_KEY_CODE, MacOsShortcut, ShortcutKey};

const MAX_KEY_CODE: i64 = 127;
const MODIFIER_KEY_CODES: [i64; 10] = [54, 55, 56, 57, 58, 59, 60, 61, 62, 63];

#[derive(Default)]
pub(super) struct UntrustedShortcutDetector {
    previously_down: HashSet<i64>,
    used_modifiers: HashSet<i64>,
    capture_armed: bool,
    capture_previously_down: HashSet<i64>,
    capture_modifier_order: Vec<i64>,
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

    pub(super) fn observe_capture_system(&mut self) -> Option<CaptureObservation> {
        // SAFETY: these CoreGraphics functions only read the current login
        // session's global event-state table and accept every key code we pass.
        let flags = CGEventFlags::from_bits_truncate(unsafe {
            CGEventSourceFlagsState(CGEventSourceStateID::CombinedSessionState)
        });
        self.observe_capture(flags, |code| unsafe {
            CGEventSourceKeyState(CGEventSourceStateID::CombinedSessionState, code as u16)
        })
    }

    fn observe_capture(
        &mut self,
        flags: CGEventFlags,
        mut key_down: impl FnMut(i64) -> bool,
    ) -> Option<CaptureObservation> {
        let current = (0..=MAX_KEY_CODE)
            .filter(|code| key_down(*code))
            .collect::<HashSet<_>>();
        if !self.capture_armed {
            self.capture_armed = current.is_empty();
            self.capture_previously_down = current;
            return None;
        }

        for code in MODIFIER_KEY_CODES {
            if current.contains(&code)
                && !self.capture_previously_down.contains(&code)
                && !self.capture_modifier_order.contains(&code)
            {
                self.capture_modifier_order.push(code);
            }
        }

        let pressed_key = (0..=MAX_KEY_CODE).find(|code| {
            !MODIFIER_KEY_CODES.contains(code)
                && current.contains(code)
                && !self.capture_previously_down.contains(code)
        });
        let released_modifier =
            self.capture_modifier_order.iter().copied().find(|code| {
                self.capture_previously_down.contains(code) && !current.contains(code)
            });
        self.capture_previously_down = current;

        match pressed_key {
            Some(ESCAPE_KEY_CODE) => Some(CaptureObservation::Cancel),
            Some(code) => Some(CaptureObservation::Shortcut(MacOsShortcut::physical(
                code, flags,
            ))),
            None => released_modifier
                .map(MacOsShortcut::modifier)
                .map(CaptureObservation::Shortcut),
        }
    }

    pub(super) fn reset_capture(&mut self) {
        self.capture_armed = false;
        self.capture_previously_down.clear();
        self.capture_modifier_order.clear();
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
    use crate::macos_shortcut_monitor::{
        ESCAPE_KEY_CODE, MacOsShortcut, MacOsShortcutController, MacOsShortcutError,
    };

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

    fn observe_capture(
        detector: &mut UntrustedShortcutDetector,
        controller: &MacOsShortcutController,
        down: &[i64],
        flags: CGEventFlags,
    ) {
        let down = down.iter().copied().collect::<HashSet<_>>();
        if let Some(observation) = detector.observe_capture(flags, |code| down.contains(&code)) {
            observation.finish(controller);
        }
    }

    #[test]
    fn capture_without_configured_shortcuts_detects_a_chord() {
        let controller = MacOsShortcutController::new(Vec::new());
        let receiver = controller
            .begin_capture()
            .unwrap_or_else(|error| panic!("capture should start: {error}"));
        let mut detector = UntrustedShortcutDetector::default();

        observe_capture(&mut detector, &controller, &[], CGEventFlags::empty());
        observe_capture(
            &mut detector,
            &controller,
            &[40, 54],
            CGEventFlags::CGEventFlagCommand,
        );

        let expected = MacOsShortcut::physical(40, CGEventFlags::CGEventFlagCommand);
        assert_eq!(Ok(Ok(expected)), receiver.recv());
    }

    #[test]
    fn capture_without_configured_shortcuts_detects_a_modifier_release() {
        let controller = MacOsShortcutController::new(Vec::new());
        let receiver = controller
            .begin_capture()
            .unwrap_or_else(|error| panic!("capture should start: {error}"));
        let mut detector = UntrustedShortcutDetector::default();

        observe_capture(&mut detector, &controller, &[], CGEventFlags::empty());
        observe_capture(
            &mut detector,
            &controller,
            &[54],
            CGEventFlags::CGEventFlagCommand,
        );
        observe_capture(&mut detector, &controller, &[], CGEventFlags::empty());

        assert_eq!(Ok(Ok(MacOsShortcut::modifier(54))), receiver.recv());
    }

    #[test]
    fn right_option_keeps_its_side_when_polling_adds_a_left_option_alias() {
        let controller = MacOsShortcutController::new(Vec::new());
        let receiver = controller
            .begin_capture()
            .unwrap_or_else(|error| panic!("capture should start: {error}"));
        let mut detector = UntrustedShortcutDetector::default();

        observe_capture(&mut detector, &controller, &[], CGEventFlags::empty());
        observe_capture(
            &mut detector,
            &controller,
            &[61],
            CGEventFlags::CGEventFlagAlternate,
        );
        observe_capture(
            &mut detector,
            &controller,
            &[58, 61],
            CGEventFlags::CGEventFlagAlternate,
        );
        observe_capture(&mut detector, &controller, &[], CGEventFlags::empty());

        let result = receiver
            .recv()
            .map(|result| result.map(|shortcut| shortcut.display_label()));
        assert_eq!(Ok(Ok("Right Option".to_owned())), result);
    }

    #[test]
    fn capture_waits_for_the_activation_key_to_be_released() {
        let controller = MacOsShortcutController::new(Vec::new());
        let receiver = controller
            .begin_capture()
            .unwrap_or_else(|error| panic!("capture should start: {error}"));
        let mut detector = UntrustedShortcutDetector::default();

        observe_capture(&mut detector, &controller, &[36], CGEventFlags::empty());
        observe_capture(&mut detector, &controller, &[], CGEventFlags::empty());
        assert!(receiver.try_recv().is_err());

        observe_capture(
            &mut detector,
            &controller,
            &[40, 54],
            CGEventFlags::CGEventFlagCommand,
        );
        assert!(receiver.recv().is_ok());
    }

    #[test]
    fn escape_cancels_capture_without_accessibility_permission() {
        let controller = MacOsShortcutController::new(Vec::new());
        let receiver = controller
            .begin_capture()
            .unwrap_or_else(|error| panic!("capture should start: {error}"));
        let mut detector = UntrustedShortcutDetector::default();

        observe_capture(&mut detector, &controller, &[], CGEventFlags::empty());
        observe_capture(
            &mut detector,
            &controller,
            &[ESCAPE_KEY_CODE],
            CGEventFlags::empty(),
        );

        assert_eq!(
            Ok(Err(MacOsShortcutError::CaptureCancelled)),
            receiver.recv()
        );
    }
}
