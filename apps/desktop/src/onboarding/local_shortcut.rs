use slint::winit_030::winit::{
    event::{ElementState, WindowEvent},
    keyboard::{KeyCode, PhysicalKey},
};

pub(super) fn event_requests_toggle(event: &WindowEvent, accessibility_authorized: bool) -> bool {
    matches!(
        event,
        WindowEvent::KeyboardInput { event, .. }
            if is_default_shortcut_release(
                &event.physical_key,
                event.state,
                event.repeat,
                accessibility_authorized,
            )
    )
}

fn is_default_shortcut_release(
    physical_key: &PhysicalKey,
    state: ElementState,
    repeat: bool,
    accessibility_authorized: bool,
) -> bool {
    !accessibility_authorized
        && *physical_key == PhysicalKey::Code(KeyCode::SuperRight)
        && state == ElementState::Released
        && !repeat
}

#[cfg(test)]
mod tests {
    use slint::winit_030::winit::{
        event::ElementState,
        keyboard::{KeyCode, PhysicalKey},
    };

    use super::is_default_shortcut_release;

    #[test]
    fn right_command_release_triggers_the_local_onboarding_test_once() {
        assert!(is_default_shortcut_release(
            &PhysicalKey::Code(KeyCode::SuperRight),
            ElementState::Released,
            false,
            false,
        ));
        assert!(!is_default_shortcut_release(
            &PhysicalKey::Code(KeyCode::SuperLeft),
            ElementState::Released,
            false,
            false,
        ));
        assert!(!is_default_shortcut_release(
            &PhysicalKey::Code(KeyCode::SuperRight),
            ElementState::Pressed,
            false,
            false,
        ));
        assert!(!is_default_shortcut_release(
            &PhysicalKey::Code(KeyCode::SuperRight),
            ElementState::Released,
            true,
            false,
        ));
        assert!(!is_default_shortcut_release(
            &PhysicalKey::Code(KeyCode::SuperRight),
            ElementState::Released,
            false,
            true,
        ));
    }
}
