use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use slint::{ComponentHandle, Timer};

#[allow(
    clippy::panic,
    clippy::todo,
    clippy::unimplemented,
    clippy::unwrap_used
)]
mod ui {
    slint::include_modules!();
}

use ui::{AppPage, AppWindow};

fn main() {
    let ui = require(AppWindow::new(), "construct app window");
    require(ui.show(), "display app window");
    ui.set_model_tab(1);
    ui.set_focus_asr_config(true);
    ui.set_current_page(AppPage::Models);

    let consumed_after_switches = Rc::new(Cell::new((false, false)));
    let first_switch = ui.as_weak();
    let first_result = Rc::clone(&consumed_after_switches);
    Timer::single_shot(Duration::ZERO, move || {
        let Some(ui) = first_switch.upgrade() else {
            fail("app window was dropped before the first tab switch");
        };
        ui.set_model_tab(0);

        let second_switch = ui.as_weak();
        let second_result = Rc::clone(&first_result);
        Timer::single_shot(Duration::ZERO, move || {
            let Some(ui) = second_switch.upgrade() else {
                fail("app window was dropped before the second tab switch");
            };
            let consumed_after_first_switch = !ui.get_focus_asr_config();
            ui.set_model_tab(1);
            ui.set_model_tab(0);
            second_result.set((consumed_after_first_switch, !ui.get_focus_asr_config()));
            let _ = slint::quit_event_loop();
        });
    });

    require(slint::run_event_loop(), "run event loop");
    assert_eq!((true, true), consumed_after_switches.get());
}

fn require<T, E: std::fmt::Display>(result: Result<T, E>, action: &str) -> T {
    match result {
        Ok(value) => value,
        Err(error) => fail(&format!("failed to {action}: {error}")),
    }
}

fn fail(message: &str) -> ! {
    eprintln!("models navigation test failed: {message}");
    std::process::exit(1);
}
