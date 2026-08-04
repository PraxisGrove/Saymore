#[cfg(target_os = "macos")]
use slint::ComponentHandle;

#[cfg(target_os = "macos")]
#[allow(
    clippy::panic,
    clippy::todo,
    clippy::unimplemented,
    clippy::unwrap_used
)]
mod ui {
    slint::include_modules!();
}

#[cfg(target_os = "macos")]
fn main() {
    let ui = require(ui::AppWindow::new());

    require(slint::select_bundled_translation("zh-Hans"));
    assert_eq!(
        "正在录音",
        ui.global::<ui::Translations>()
            .get_recording_active()
            .as_str()
    );
    assert_eq!(
        "已保存 1 个词条",
        ui.global::<ui::Translations>()
            .invoke_dictionary_saved(1)
            .as_str()
    );
    assert_eq!(
        "豆包流式语音识别模型 1.0",
        ui.global::<ui::Translations>()
            .get_history_model_volcengine_v1()
            .as_str()
    );
    assert_eq!(
        "豆包流式语音识别模型 2.0",
        ui.global::<ui::Translations>()
            .get_history_model_volcengine_v2()
            .as_str()
    );
    assert_eq!(
        "macOS 听写",
        ui.global::<ui::Translations>()
            .get_history_model_macos_dictation()
            .as_str()
    );
    assert_eq!(
        "Whisper Turbo",
        ui.global::<ui::Translations>()
            .get_history_model_whisper_turbo()
            .as_str()
    );
    assert_eq!(
        "豆包 1.0",
        ui.global::<ui::Translations>()
            .get_history_model_volcengine_v1_short()
            .as_str()
    );
    assert_eq!(
        "豆包 2.0",
        ui.global::<ui::Translations>()
            .get_history_model_volcengine_v2_short()
            .as_str()
    );

    require(slint::select_bundled_translation("en"));
    assert_eq!(
        "Listening",
        ui.global::<ui::Translations>()
            .get_recording_active()
            .as_str()
    );
    assert_eq!(
        "Saved 1 entry",
        ui.global::<ui::Translations>()
            .invoke_dictionary_saved(1)
            .as_str()
    );
    assert_eq!(
        "Saved 2 entries",
        ui.global::<ui::Translations>()
            .invoke_dictionary_saved(2)
            .as_str()
    );
    assert_eq!(
        "Doubao Streaming Speech Recognition 1.0",
        ui.global::<ui::Translations>()
            .get_history_model_volcengine_v1()
            .as_str()
    );
    assert_eq!(
        "Doubao Streaming Speech Recognition 2.0",
        ui.global::<ui::Translations>()
            .get_history_model_volcengine_v2()
            .as_str()
    );
    assert_eq!(
        "macOS Dictation",
        ui.global::<ui::Translations>()
            .get_history_model_macos_dictation()
            .as_str()
    );
    assert_eq!(
        "Whisper Turbo",
        ui.global::<ui::Translations>()
            .get_history_model_whisper_turbo()
            .as_str()
    );
    assert_eq!(
        "Doubao 1.0",
        ui.global::<ui::Translations>()
            .get_history_model_volcengine_v1_short()
            .as_str()
    );
    assert_eq!(
        "Doubao 2.0",
        ui.global::<ui::Translations>()
            .get_history_model_volcengine_v2_short()
            .as_str()
    );
}

#[cfg(not(target_os = "macos"))]
fn main() {}

#[cfg(target_os = "macos")]
fn require<T, E: std::fmt::Display>(result: Result<T, E>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => {
            eprintln!("i18n runtime test failed: {error}");
            std::process::exit(1);
        }
    }
}
