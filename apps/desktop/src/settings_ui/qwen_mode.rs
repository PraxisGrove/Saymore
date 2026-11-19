use crate::ui::{AppWindow, AsrProviderCardKind, LocalRecognitionMode as UiMode};
use slint::{ComponentHandle, SharedString};
use std::sync::Arc;
use template_app::{ProviderConfigStore, QwenRecognitionMode};
use template_infra::JsonSettingsStore;

pub(super) fn wire(ui: &AppWindow, store: Arc<JsonSettingsStore>) {
    match store
        .load_catalog()
        .and_then(|catalog| catalog.qwen_recognition_mode())
    {
        Ok(mode) => ui.set_qwen_recognition_mode(to_ui(mode)),
        Err(error) => ui
            .invoke_report_local_operation(AsrProviderCardKind::Qwen3Asr, error.to_string().into()),
    }
    let weak = ui.as_weak();
    ui.on_change_qwen_recognition_mode(move |mode| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        let selected = match mode {
            UiMode::Streaming => QwenRecognitionMode::Streaming,
            UiMode::NonStreaming => QwenRecognitionMode::NonStreaming,
        };
        match store.set_qwen_recognition_mode(selected) {
            Ok(()) => {
                ui.set_qwen_recognition_mode(mode);
                ui.invoke_report_local_operation(
                    AsrProviderCardKind::Qwen3Asr,
                    SharedString::default(),
                );
            }
            Err(error) => ui.invoke_report_local_operation(
                AsrProviderCardKind::Qwen3Asr,
                error.to_string().into(),
            ),
        }
    });
}

fn to_ui(mode: QwenRecognitionMode) -> UiMode {
    match mode {
        QwenRecognitionMode::Streaming => UiMode::Streaming,
        QwenRecognitionMode::NonStreaming => UiMode::NonStreaming,
    }
}
