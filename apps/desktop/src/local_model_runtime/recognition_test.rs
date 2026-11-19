use super::*;

use crate::ui::{LocalModelTestState, LocalModelTestStatus};

pub(super) fn running(ui: &AppWindow, model: LocalModel) -> bool {
    let state = match model {
        LocalModel::Whisper => ui.get_whisper_test(),
        LocalModel::Qwen3 => ui.get_qwen3_asr_test(),
        LocalModel::Seaco => ui.get_seaco_test(),
        LocalModel::Zipformer => ui.get_zipformer_test(),
    };
    state.status == LocalModelTestStatus::Running
}

pub(super) fn set_state(ui: &AppWindow, model: LocalModel, state: LocalModelTestState) {
    match model {
        LocalModel::Whisper => ui.set_whisper_test(state),
        LocalModel::Qwen3 => ui.set_qwen3_asr_test(state),
        LocalModel::Seaco => ui.set_seaco_test(state),
        LocalModel::Zipformer => ui.set_zipformer_test(state),
    }
}

pub(super) fn wire(
    ui: &AppWindow,
    installers: Arc<LocalModelInstallers>,
    asr: Arc<AsrSessionController>,
) {
    let weak = ui.as_weak();
    ui.on_request_local_model_test(move |kind| {
        let Some((model, installer)) = installer_for(kind, &installers) else {
            return;
        };
        if !installer.is_installed() {
            return;
        }
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if activation_pending(&ui, model) || running(&ui, model) {
            return;
        }
        set_state(
            &ui,
            model,
            LocalModelTestState {
                status: LocalModelTestStatus::Running,
                ..Default::default()
            },
        );
        let result_ui = weak.clone();
        let asr = Arc::clone(&asr);
        let spawn = std::thread::Builder::new()
            .name(model.thread_name("test"))
            .spawn(move || {
                let result = run_test(model, &asr);
                let _ = result_ui.upgrade_in_event_loop(move |ui| {
                    let (elapsed, transcript) = match result {
                        Ok((elapsed, transcript)) => {
                            (format!("{:.2}", elapsed.as_secs_f64()), transcript)
                        }
                        Err(error) => (String::new(), Err(error)),
                    };
                    let (status, transcript) = match transcript {
                        Ok(text) => (LocalModelTestStatus::Succeeded, text),
                        Err(error) => (LocalModelTestStatus::Failed, error.to_string()),
                    };
                    set_state(
                        &ui,
                        model,
                        LocalModelTestState {
                            status,
                            elapsed: elapsed.into(),
                            transcript: transcript.into(),
                        },
                    );
                });
            });
        if let Err(error) = spawn {
            set_state(
                &ui,
                model,
                LocalModelTestState {
                    status: LocalModelTestStatus::Failed,
                    transcript: error.to_string().into(),
                    ..Default::default()
                },
            );
        }
    });
}

fn run_test(
    model: LocalModel,
    asr: &AsrSessionController,
) -> Result<
    (
        std::time::Duration,
        Result<String, template_app::SpeechRecognitionError>,
    ),
    template_app::SpeechRecognitionError,
> {
    match model {
        LocalModel::Whisper => asr
            .whisper_recognizer()
            .map(|recognizer| settings_ui::run_local_asr_test(recognizer.as_ref())),
        LocalModel::Qwen3 => asr
            .qwen3_recognizer()
            .map(|recognizer| settings_ui::run_local_asr_test(recognizer.as_ref())),
        LocalModel::Seaco => asr
            .seaco_recognizer()
            .map(|recognizer| settings_ui::run_local_asr_test(recognizer.as_ref())),
        LocalModel::Zipformer => asr
            .zipformer_recognizer()
            .map(|recognizer| settings_ui::run_local_asr_test(recognizer.as_ref())),
    }
}
