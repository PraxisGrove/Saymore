use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use slint::platform::{PointerEventButton, WindowEvent};
use slint::{ComponentHandle, ModelRc, Timer, VecModel};

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

    let observed_state = Rc::new(Cell::new((false, false, false)));
    let first_switch = ui.as_weak();
    let first_result = Rc::clone(&observed_state);
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
            ui.set_seaco_selected(true);
            ui.set_seaco_runtime_memory_live(true);
            ui.set_seaco_runtime_memory_usage("1200 MB".into());
            assert!(ui.get_seaco_runtime_memory_active());
            ui.set_seaco_selected(false);
            second_result.set((
                consumed_after_first_switch,
                !ui.get_focus_asr_config(),
                !ui.get_seaco_runtime_memory_active(),
            ));
            check_additional_provider_actions(&ui);
            let _ = slint::quit_event_loop();
        });
    });

    require(slint::run_event_loop(), "run event loop");
    assert_eq!((true, true, true), observed_state.get());
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

fn click(ui: &AppWindow, x: f32, y: f32) {
    for event in [
        WindowEvent::PointerPressed {
            position: slint::LogicalPosition::new(x, y),
            button: PointerEventButton::Left,
        },
        WindowEvent::PointerReleased {
            position: slint::LogicalPosition::new(x, y),
            button: PointerEventButton::Left,
        },
    ] {
        require(
            ui.window().try_dispatch_event(event),
            "dispatch model action",
        );
    }
}

fn check_additional_provider_actions(ui: &AppWindow) {
    use ui::{AsrProviderCardKind as Kind, AsrProviderCardSpec, LocalModelDownloadState};
    let actions = Rc::new(RefCell::new(Vec::new()));
    let imports = Rc::clone(&actions);
    ui.on_request_seaco_import(move || {
        imports.borrow_mut().push(("import", Kind::SeacoParaformer))
    });
    let downloads = Rc::clone(&actions);
    ui.on_request_local_model_download(move |kind| downloads.borrow_mut().push(("download", kind)));
    let selections = Rc::clone(&actions);
    ui.on_request_local_model_selection(move |kind| selections.borrow_mut().push(("select", kind)));
    ui.set_focus_asr_config(false);
    ui.set_model_tab(0);
    ui.set_seaco_import_supported(true);
    ui.set_zipformer_download_supported(true);
    ui.set_zipformer_adapter_ready(true);
    ui.set_moonshine_download_supported(true);
    ui.set_asr_provider_cards(ModelRc::new(VecModel::from(vec![
        AsrProviderCardSpec {
            kind: Kind::SeacoParaformer,
            available: true,
        },
        AsrProviderCardSpec {
            kind: Kind::Zipformer,
            available: true,
        },
        AsrProviderCardSpec {
            kind: Kind::Moonshine,
            available: true,
        },
    ])));

    // The clicks below target the default 920 x 700 layout. Pin that size, since
    // the OS shrinks the window to fit small screens such as CI runner displays.
    require(
        ui.window().try_dispatch_event(WindowEvent::Resized {
            size: slint::LogicalSize::new(920.0, 700.0),
        }),
        "pin the default window size",
    );
    // The navigation check opened the cloud configuration dialog; close it first.
    click(ui, 735.0, 150.0);
    // Use the real card/modal event path at the default 920 x 700 window size.
    click(ui, 350.0, 200.0);
    click(ui, 680.0, 350.0);
    click(ui, 735.0, 150.0);
    click(ui, 700.0, 200.0);
    click(ui, 680.0, 350.0);
    ui.set_zipformer_download_state(LocalModelDownloadState::Downloaded);
    click(ui, 650.0, 150.0);
    click(ui, 735.0, 150.0);
    click(ui, 350.0, 320.0);
    click(ui, 680.0, 350.0);
    assert_eq!(
        vec![
            ("import", Kind::SeacoParaformer),
            ("download", Kind::Zipformer),
            ("select", Kind::Zipformer),
            ("download", Kind::Moonshine),
        ],
        *actions.borrow()
    );
}
