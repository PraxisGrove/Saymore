use super::*;

pub(super) fn wire(
    ui: &AppWindow,
    settings: Arc<JsonSettingsStore>,
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
        if activation_pending(&ui, model) {
            return;
        }
        ui.set_asr_testing(true);
        ui.set_asr_test_succeeded(false);
        ui.set_asr_draft_error(false);
        let result_ui = weak.clone();
        let asr = Arc::clone(&asr);
        let settings = Arc::clone(&settings);
        let spawn = std::thread::Builder::new()
            .name(model.thread_name("test"))
            .spawn(move || {
                let result = run_test(model, &settings, &asr);
                let _ = result_ui.upgrade_in_event_loop(move |ui| {
                    ui.set_asr_testing(false);
                    match result {
                        Ok((elapsed, Ok(transcript))) => {
                            ui.set_asr_test_succeeded(true);
                            ui.set_asr_test_elapsed(format!("{:.2}", elapsed.as_secs_f64()).into());
                            ui.set_asr_test_result(transcript.into());
                        }
                        Ok((elapsed, Err(error))) => {
                            ui.set_asr_test_elapsed(format!("{:.2}", elapsed.as_secs_f64()).into());
                            apply_operation_failure(&ui, error.to_string());
                        }
                        Err(error) => apply_operation_failure(&ui, error.to_string()),
                    }
                });
            });
        if let Err(error) = spawn {
            ui.set_asr_testing(false);
            apply_operation_failure(&ui, error.to_string());
        }
    });
}

fn run_test(
    model: LocalModel,
    settings: &JsonSettingsStore,
    asr: &AsrSessionController,
) -> Result<
    (
        std::time::Duration,
        Result<String, template_app::SpeechRecognitionError>,
    ),
    template_app::SpeechRecognitionError,
> {
    let mut result = match model {
        LocalModel::Paraformer => asr
            .paraformer_recognizer()
            .map(|recognizer| settings_ui::run_local_asr_test(recognizer.as_ref())),
        LocalModel::Whisper => asr
            .whisper_recognizer()
            .map(|recognizer| settings_ui::run_local_asr_test(recognizer.as_ref())),
        LocalModel::Qwen3 => asr
            .qwen3_recognizer()
            .map(|recognizer| settings_ui::run_local_asr_test(recognizer.as_ref())),
        LocalModel::SenseVoice => asr
            .sense_voice_recognizer()
            .map(|recognizer| settings_ui::run_local_asr_test(recognizer.as_ref())),
    };
    let local_punctuation = model == LocalModel::Paraformer
        && settings.load_catalog().is_ok_and(|catalog| {
            catalog.paraformer_punctuation_mode() == template_app::ParaformerPunctuationMode::Local
        });
    if local_punctuation && let Ok((elapsed, Ok(transcript))) = &result {
        result = Ok((*elapsed, Ok(asr.restore_punctuation(transcript.clone()))));
    }
    result
}
