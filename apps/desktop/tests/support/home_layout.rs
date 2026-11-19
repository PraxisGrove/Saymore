use super::{AppPage, AppWindow, click, require};
use slint::ComponentHandle;
use slint::platform::WindowEvent;
use std::cell::Cell;
use std::rc::Rc;

pub fn check_home_layout(ui: &AppWindow) {
    ui.set_current_page(AppPage::Home);
    ui.set_usage_error(true);
    let reloads = Rc::new(Cell::new(0));
    let observed = Rc::clone(&reloads);
    ui.on_refresh_usage(move || observed.set(observed.get() + 1));

    let settings_action = Rc::new(Cell::new(None));
    let microphone = Rc::clone(&settings_action);
    ui.on_open_microphone_settings(move || microphone.set(Some("microphone")));
    let delivery = Rc::clone(&settings_action);
    ui.on_open_accessibility_settings(move || delivery.set(Some("delivery")));

    for (width, height, mic_x) in [(820.0, 560.0, 670.0), (920.0, 700.0, 744.0)] {
        require(
            ui.window().try_dispatch_event(WindowEvent::Resized {
                size: slint::LogicalSize::new(width, height),
            }),
            "resize home page",
        );
        click(ui, mic_x, 227.0);
        assert_eq!(
            settings_action.take(),
            Some("microphone"),
            "microphone icon"
        );
        click(ui, width - 68.0, 266.0);
        assert_eq!(settings_action.take(), Some("delivery"), "delivery caption");
        let before = reloads.get();
        click(ui, width - 60.0, 351.0);
        assert_eq!(reloads.get(), before + 1, "home retry is reachable");
        require(
            ui.window()
                .try_dispatch_event(WindowEvent::PointerScrolled {
                    position: slint::LogicalPosition::new(width - 60.0, 500.0),
                    delta_x: 0.0,
                    delta_y: -1000.0,
                }),
            "scroll home to the end",
        );
        let scroll_offset = (616.0_f32 - (height - 20.0)).max(0.0);
        click(ui, width - 60.0, 351.0 - scroll_offset);
        assert_eq!(reloads.get(), before + 2, "retry follows scrolled content");
        require(
            ui.window()
                .try_dispatch_event(WindowEvent::PointerScrolled {
                    position: slint::LogicalPosition::new(width - 60.0, 500.0),
                    delta_x: 0.0,
                    delta_y: 1000.0,
                }),
            "scroll home back to the beginning",
        );
    }
}
