use crate::ui::{AppWindow, LocalRecognitionMode as UiMode};
use slint::{ComponentHandle, SharedString};
use std::sync::Arc;
use template_app::{ProviderConfigStore, WhisperRecognitionMode};
use template_infra::JsonSettingsStore;

pub(super) fn wire(ui: &AppWindow, store: Arc<JsonSettingsStore>) {
    match store
        .load_catalog()
        .and_then(|catalog| catalog.whisper_recognition_mode())
    {
        Ok(mode) => ui.set_whisper_recognition_mode(to_ui(mode)),
        Err(error) => ui.set_local_model_operation_error(error.to_string().into()),
    }
    let weak = ui.as_weak();
    ui.on_change_whisper_recognition_mode(move |mode| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        let selected = match mode {
            UiMode::Streaming => WhisperRecognitionMode::Streaming,
            UiMode::NonStreaming => WhisperRecognitionMode::NonStreaming,
        };
        match store.set_whisper_recognition_mode(selected) {
            Ok(()) => {
                ui.set_whisper_recognition_mode(mode);
                ui.set_local_model_operation_error(SharedString::default());
            }
            Err(error) => ui.set_local_model_operation_error(error.to_string().into()),
        }
    });
}

fn to_ui(mode: WhisperRecognitionMode) -> UiMode {
    match mode {
        WhisperRecognitionMode::Streaming => UiMode::Streaming,
        WhisperRecognitionMode::NonStreaming => UiMode::NonStreaming,
    }
}
