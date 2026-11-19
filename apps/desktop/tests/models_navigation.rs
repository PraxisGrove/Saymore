use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use slint::platform::WindowEvent;
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
#[path = "support/model_controls.rs"]
mod controls;
use controls::{
    AccessibleRole::{Button, RadioButton, Text},
    bounds, click as press, click_at as click, visible_bounds,
};

#[path = "support/home_layout.rs"]
mod home_layout;
#[path = "support/polishing_consent.rs"]
mod polishing_consent;

fn main() {
    let ui = require(AppWindow::new(), "construct app window");
    require(
        slint::select_bundled_translation("en"),
        "select translation",
    );
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
            home_layout::check_home_layout(&ui);
            polishing_consent::check_slider_while_confirming(ui);
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
    ui.set_seaco_download_supported(true);
    ui.set_zipformer_download_supported(true);
    ui.set_zipformer_adapter_ready(true);
    ui.set_asr_provider_cards(ModelRc::new(VecModel::from(vec![
        AsrProviderCardSpec {
            kind: Kind::SeacoParaformer,
            available: true,
        },
        AsrProviderCardSpec {
            kind: Kind::Zipformer,
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
    // Dismiss the cloud configuration opened by the navigation check.
    press(ui, Button, "Close");
    press(ui, Button, "SeACo-Paraformer");
    press(ui, Button, "Download");
    press(ui, Button, "Import model");
    ui.set_seaco_download_state(LocalModelDownloadState::Downloaded);
    bounds(ui, Text, "Non-streaming");
    assert!(visible_bounds(ui, RadioButton, "Streaming").is_none());
    press(ui, Button, "Close");
    press(ui, Button, "Zipformer");
    assert!(visible_bounds(ui, Button, "Import model").is_none());
    press(ui, Button, "Download");
    ui.set_zipformer_download_state(LocalModelDownloadState::Downloaded);
    bounds(ui, Text, "Streaming");
    assert!(visible_bounds(ui, RadioButton, "Streaming").is_none());

    // A pending result for a different local model must not leak into the current dialog.
    ui.set_seaco_test(ui::LocalModelTestState {
        status: ui::LocalModelTestStatus::Succeeded,
        transcript: "SeACo sample".into(),
        ..Default::default()
    });
    ui.invoke_report_local_operation(Kind::SeacoParaformer, "SeACo activation failed".into());
    assert!(visible_bounds(ui, Text, "Transcript: SeACo sample").is_none());
    assert!(visible_bounds(ui, Text, "SeACo activation failed").is_none());
    press(ui, Button, "Use model");
    press(ui, Button, "Close");
    press(ui, Button, "SeACo-Paraformer");
    bounds(ui, Text, "Transcript: SeACo sample");
    bounds(ui, Text, "SeACo activation failed");
    press(ui, Button, "Close");
    assert_eq!(
        vec![
            ("download", Kind::SeacoParaformer),
            ("import", Kind::SeacoParaformer),
            ("download", Kind::Zipformer),
            ("select", Kind::Zipformer),
        ],
        *actions.borrow()
    );
}
