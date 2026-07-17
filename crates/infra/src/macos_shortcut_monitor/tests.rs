use super::*;
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

fn modifier_event(code: i64, flags: CGEventFlags) -> CGEvent {
    let Ok(source) = CGEventSource::new(CGEventSourceStateID::Private) else {
        panic!("a private event source should be available");
    };
    let Ok(event) = CGEvent::new_keyboard_event(source, code as u16, true) else {
        panic!("a synthetic modifier event should be available");
    };
    event.set_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE, code);
    event.set_flags(flags);
    event
}

#[test]
fn supports_multiple_shortcuts_and_rejects_duplicates() {
    let fn_key = MacOsShortcut::modifier(63);
    let command_a = MacOsShortcut::from_capture("A", true, false, false, false);
    let Ok(command_a) = command_a else {
        panic!("Command-A should be representable");
    };
    let controller = MacOsShortcutController::new(vec![fn_key.clone(), command_a]);
    assert_eq!(
        2,
        controller
            .current()
            .map(|items| items.len())
            .unwrap_or_default()
    );
    assert_eq!(
        Err(MacOsShortcutError::Duplicate),
        controller.replace(vec![fn_key.clone(), fn_key])
    );
}

#[test]
fn reserves_fn_only_while_bound_or_capturing() {
    let fn_controller = MacOsShortcutController::new(vec![MacOsShortcut::modifier(63)]);
    assert!(fn_controller.reserves_fn());

    let command_controller = MacOsShortcutController::new(vec![MacOsShortcut::modifier(54)]);
    assert!(!command_controller.reserves_fn());
    let Ok(_capture) = command_controller.begin_capture() else {
        panic!("shortcut capture should start");
    };
    assert!(command_controller.reserves_fn());
}

#[test]
fn round_trips_physical_and_modifier_shortcuts() {
    for shortcut in [
        MacOsShortcut::modifier(63),
        MacOsShortcut::modifier(54),
        MacOsShortcut::physical(123, CGEventFlags::CGEventFlagCommand),
    ] {
        assert_eq!(
            Ok(shortcut.clone()),
            MacOsShortcut::from_storage_value(&shortcut.storage_value())
        );
    }
}

#[test]
fn round_trips_fn_physical_key_combinations() {
    let shortcut = MacOsShortcut::physical(49, CGEventFlags::CGEventFlagSecondaryFn);

    assert_eq!("fn+key-49", shortcut.storage_value());
    assert_eq!("Fn Space", shortcut.display_label());
    assert_eq!(
        Ok(shortcut.clone()),
        MacOsShortcut::from_storage_value(&shortcut.storage_value())
    );
}

#[test]
fn rejects_known_system_shortcuts() {
    assert_eq!(
        Err(MacOsShortcutError::SystemReserved),
        MacOsShortcut::from_capture(" ", true, false, false, false)
    );
    assert!(!MacOsShortcut::modifier(63).likely_system_conflict());
}

#[test]
fn rejects_physical_keys_without_a_non_shift_modifier() {
    for (text, shift) in [("A", false), ("1", false), (" ", false), ("A", true)] {
        assert_eq!(
            Err(MacOsShortcutError::MissingModifier),
            MacOsShortcut::from_capture(text, false, false, false, shift)
        );
    }
}

#[test]
fn captured_physical_keys_use_the_same_modifier_validation() {
    let controller = MacOsShortcutController::new(Vec::new());
    let Ok(capture) = controller.begin_capture() else {
        panic!("shortcut capture should start");
    };

    controller.finish_capture(MacOsShortcut::physical(0, CGEventFlags::empty()));

    assert_eq!(Ok(Err(MacOsShortcutError::MissingModifier)), capture.recv());
}

#[test]
fn consumes_escape_only_while_recording() {
    assert!(!cancel_shortcut(ESCAPE_KEY_CODE, false));
    assert!(cancel_shortcut(ESCAPE_KEY_CODE, true));
    assert!(!cancel_shortcut(RIGHT_COMMAND_KEY_CODE, true));
}

#[test]
fn bound_fn_press_and_release_trigger_dictation_without_reaching_macos() {
    let controller = MacOsShortcutController::new(vec![MacOsShortcut::modifier(63)]);
    let modifier_state = Mutex::new(ModifierState::default());
    let (sender, receiver) = channel();

    let pressed = modifier_event(63, CGEventFlags::CGEventFlagSecondaryFn);
    assert!(matches!(
        handle_modifier_event(&pressed, &modifier_state, &controller, &sender),
        CallbackResult::Drop
    ));

    let released = modifier_event(63, CGEventFlags::empty());
    assert!(matches!(
        handle_modifier_event(&released, &modifier_state, &controller, &sender),
        CallbackResult::Drop
    ));
    assert_eq!(Ok(DictationShortcutAction::Toggle), receiver.recv());
}

#[test]
fn captured_fn_press_and_release_are_saved_without_reaching_macos() {
    let controller = MacOsShortcutController::new(Vec::new());
    let Ok(capture) = controller.begin_capture() else {
        panic!("shortcut capture should start");
    };
    let modifier_state = Mutex::new(ModifierState::default());
    let (sender, _receiver) = channel();

    let pressed = modifier_event(63, CGEventFlags::CGEventFlagSecondaryFn);
    assert!(matches!(
        handle_modifier_event(&pressed, &modifier_state, &controller, &sender),
        CallbackResult::Drop
    ));

    let released = modifier_event(63, CGEventFlags::empty());
    assert!(matches!(
        handle_modifier_event(&released, &modifier_state, &controller, &sender),
        CallbackResult::Drop
    ));
    assert_eq!(Ok(Ok(MacOsShortcut::modifier(63))), capture.recv());
}

#[test]
fn bound_fn_used_in_a_chord_does_not_trigger_dictation_or_macos_fn_action() {
    let controller = MacOsShortcutController::new(vec![MacOsShortcut::modifier(63)]);
    let modifier_state = Mutex::new(ModifierState::default());
    let (sender, receiver) = channel();

    let pressed = modifier_event(63, CGEventFlags::CGEventFlagSecondaryFn);
    assert!(matches!(
        handle_modifier_event(&pressed, &modifier_state, &controller, &sender),
        CallbackResult::Drop
    ));
    mark_active_modifiers_used(&modifier_state);

    let released = modifier_event(63, CGEventFlags::empty());
    assert!(matches!(
        handle_modifier_event(&released, &modifier_state, &controller, &sender),
        CallbackResult::Drop
    ));
    assert!(receiver.try_recv().is_err());
}

#[test]
fn unbound_fn_events_continue_to_macos() {
    let controller = MacOsShortcutController::new(vec![MacOsShortcut::modifier(54)]);
    let modifier_state = Mutex::new(ModifierState::default());
    let (sender, receiver) = channel();

    let pressed = modifier_event(63, CGEventFlags::CGEventFlagSecondaryFn);
    assert!(matches!(
        handle_modifier_event(&pressed, &modifier_state, &controller, &sender),
        CallbackResult::Keep
    ));

    let released = modifier_event(63, CGEventFlags::empty());
    assert!(matches!(
        handle_modifier_event(&released, &modifier_state, &controller, &sender),
        CallbackResult::Keep
    ));
    assert!(receiver.try_recv().is_err());
}

#[test]
fn escape_can_cancel_an_active_capture() {
    let controller = MacOsShortcutController::new(Vec::new());
    let receiver = controller.begin_capture();
    let Ok(receiver) = receiver else {
        panic!("capture should start");
    };
    controller.cancel_capture();
    assert_eq!(
        Ok(Err(MacOsShortcutError::CaptureCancelled)),
        receiver.recv()
    );
    assert!(!controller.capturing());
}

#[test]
fn capture_does_not_replace_the_configured_shortcut() {
    let original = MacOsShortcut::modifier(54);
    let replacement = MacOsShortcut::modifier(63);
    let controller = MacOsShortcutController::new(vec![original.clone()]);
    let Ok(receiver) = controller.begin_capture() else {
        panic!("capture should start");
    };

    assert_eq!(Ok(vec![original.clone()]), controller.current());
    controller.finish_capture(replacement.clone());
    assert_eq!(Ok(Ok(replacement)), receiver.recv());
    assert_eq!(Ok(vec![original]), controller.current());
}
