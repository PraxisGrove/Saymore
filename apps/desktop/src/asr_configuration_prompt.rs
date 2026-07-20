use slint::ComponentHandle;

use crate::{
    overlay_window, status_tray,
    ui::{AppPage, AppWindow, AsrConfigurationOverlay},
};

pub(crate) fn wire(ui: &AppWindow, prompt: &AsrConfigurationOverlay) {
    let dismiss_prompt = prompt.as_weak();
    prompt.on_dismiss(move || hide(&dismiss_prompt));

    let configuration_prompt = prompt.as_weak();
    let configuration_ui = ui.as_weak();
    prompt.on_open_configuration(move || {
        hide(&configuration_prompt);
        if let Some(ui) = configuration_ui.upgrade() {
            ui.set_focus_asr_config(true);
            ui.set_current_page(AppPage::Models);
        }
        status_tray::show_window(&configuration_ui, None);
    });
}

pub(crate) fn show(prompt: &slint::Weak<AsrConfigurationOverlay>) {
    let Some(prompt) = prompt.upgrade() else {
        return;
    };
    if let Err(error) = overlay_window::present(prompt.window()) {
        tracing::warn!(event = "asr.configuration_prompt_present_failed", reason = %error);
    }
}

fn hide(prompt: &slint::Weak<AsrConfigurationOverlay>) {
    if let Some(prompt) = prompt.upgrade() {
        let _ = prompt.hide();
    }
}
