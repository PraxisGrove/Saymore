use std::{cell::RefCell, rc::Rc};

use crate::{controls, require, settle, ui, wait_for_discovery};

pub fn check(ui: &ui::AppWindow) {
    require(slint::select_bundled_translation("en"));
    ui.set_llm_modal_visible(false);
    ui.set_asr_config_dirty(true);
    ui.set_custom_asr_model("manual-model".into());
    settle();

    let submitted = Rc::new(RefCell::new(None));
    let observed = Rc::clone(&submitted);
    ui.on_save_asr_config(move |_, _, _, model| {
        *observed.borrow_mut() = Some(model.to_string());
    });
    let submit = || controls::click(ui, controls::AccessibleRole::Button, "Test and save");
    ui.invoke_refresh_models();
    assert!(ui.get_model_discovery_loading());
    submit();
    assert!(submitted.borrow().is_none());

    wait_for_discovery(ui);
    assert_eq!("model-a", ui.get_custom_asr_model().as_str());
    submit();
    assert_eq!(Some("model-a"), submitted.borrow().as_deref());
}
