use super::{AppPage, AppWindow, click, require};
use slint::ComponentHandle;
use slint::platform::WindowEvent;
use std::cell::Cell;
use std::rc::Rc;

pub fn check_home_layout(ui: &AppWindow) {
    // Close the model dialog left by the provider action checks.
    click(ui, 735.0, 150.0);
    ui.set_current_page(AppPage::Home);
    ui.set_usage_error(true);
    let reloads = Rc::new(Cell::new(0));
    let observed = Rc::clone(&reloads);
    ui.on_refresh_usage(move || observed.set(observed.get() + 1));

    for (width, height) in [(820.0, 560.0), (920.0, 700.0)] {
        require(
            ui.window().try_dispatch_event(WindowEvent::Resized {
                size: slint::LogicalSize::new(width, height),
            }),
            "resize home page",
        );
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
