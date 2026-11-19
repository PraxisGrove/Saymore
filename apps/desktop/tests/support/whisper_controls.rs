use crate::controls::{AccessibleRole::Button, bounds};
use crate::{support, ui};
use slint::{
    ComponentHandle, Timer,
    platform::{PointerEventButton, WindowEvent},
    private_unstable_api::re_exports::{ItemRc, StyledTextItem, WindowInner, WindowKind},
};

pub fn use_scripted_pointer(window: &slint::Window) {
    use slint::winit_030::{EventResult, WinitWindowAccessor, winit::event::WindowEvent};

    // A real cursor moving over this test window can cancel Slint’s native
    // hover timer. Keep the injected pointer as its sole pointer input.
    window.on_winit_window_event(|_, event| match event {
        WindowEvent::CursorMoved { .. }
        | WindowEvent::CursorLeft { .. }
        | WindowEvent::MouseInput { .. }
        | WindowEvent::MouseWheel { .. } => EventResult::PreventDefault,
        _ => EventResult::Propagate,
    });
}

pub fn check_remove_gesture(window: &ui::AppWindow) {
    let trash = bounds(window, Button, "Remove model");
    let icon = slint::LogicalPosition::new(trash.center().x, trash.center().y);
    let outside = slint::LogicalPosition::new(trash.min_x() - 8.0, icon.y);
    let press = WindowEvent::PointerPressed {
        position: icon,
        button: PointerEventButton::Left,
    };
    let release = |position| WindowEvent::PointerReleased {
        position,
        button: PointerEventButton::Left,
    };
    let send = |event| support::require(window.window().try_dispatch_event(event).is_ok());

    send(press.clone());
    assert_eq!(
        window.get_whisper_download_state(),
        ui::LocalModelDownloadState::Downloaded
    );
    send(WindowEvent::PointerMoved { position: outside });
    send(release(outside));
    assert_eq!(
        window.get_whisper_download_state(),
        ui::LocalModelDownloadState::Downloaded
    );

    send(WindowEvent::PointerMoved { position: icon });
    send(press);
    assert_eq!(
        window.get_whisper_download_state(),
        ui::LocalModelDownloadState::Downloaded
    );
    send(release(icon));
    assert_eq!(
        window.get_whisper_download_state(),
        ui::LocalModelDownloadState::NotDownloaded
    );
}

pub fn check_remove_tooltip(window: ui::AppWindow, done: impl FnOnce(ui::AppWindow) + 'static) {
    // The test expands the result and switches modes in a single event. Let
    // Slint finish laying out the installed card before moving the pointer.
    Timer::single_shot(std::time::Duration::from_millis(100), move || {
        hover_remove(window, done);
    });
}

fn hover_remove(window: ui::AppWindow, done: impl FnOnce(ui::AppWindow) + 'static) {
    let point = bounds(&window, Button, "Remove model").center();
    let pointer = WindowEvent::PointerMoved {
        position: slint::LogicalPosition::new(point.x, point.y),
    };
    support::require(window.window().try_dispatch_event(pointer).is_ok());
    // The native Slint tooltip opens after 500 ms of hover.
    Timer::single_shot(std::time::Duration::from_millis(750), move || {
        let inner = WindowInner::from_pub(window.window());
        let expected = slint::StyledText::from_plain_text("Remove model");
        let popups = inner.active_popups();
        let found = popups.iter().any(|popup| {
            matches!(popup.window_kind, WindowKind::ToolTip)
                && tooltip_contains(ItemRc::new_root(popup.component.clone()), &expected)
        });
        assert!(
            found,
            "Hovering the trash icon must show the Remove model tooltip ({} popups at {:?})",
            popups.len(),
            point
        );
        drop(popups);
        assert_eq!(
            window.get_whisper_download_state(),
            ui::LocalModelDownloadState::Downloaded
        );
        support::require(
            window
                .window()
                .try_dispatch_event(WindowEvent::PointerExited)
                .is_ok(),
        );
        assert!(
            inner
                .active_popups()
                .iter()
                .all(|popup| !matches!(popup.window_kind, WindowKind::ToolTip)),
            "Leaving the trash icon must dismiss the tooltip"
        );
        done(window);
    });
}

fn tooltip_contains(item: ItemRc, expected: &slint::StyledText) -> bool {
    if let Some(styled) = item.downcast::<StyledTextItem>() {
        let text = StyledTextItem::FIELD_OFFSETS
            .text()
            .apply_pin(styled.as_pin_ref())
            .get();
        return text == *expected;
    }
    std::iter::successors(item.first_child(), ItemRc::next_sibling)
        .any(|child| tooltip_contains(child, expected))
}
