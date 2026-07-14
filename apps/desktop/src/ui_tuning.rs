use std::env;

use template_infra::copy_text_to_clipboard;

use crate::ui::AppWindow;

const UI_TUNING_FLAG: &str = "--ui-tuning";
const UI_TUNING_ENV: &str = "SAYMORE_UI_TUNING";

pub fn wire(ui: &AppWindow) {
    let requested = env::args().any(|argument| argument == UI_TUNING_FLAG)
        || env::var_os(UI_TUNING_ENV).is_some();
    ui.set_ui_tuning_available(cfg!(debug_assertions) || requested);
    ui.on_copy_ui_tuning(|config| copy_text_to_clipboard(config.as_str()).is_ok());
}
