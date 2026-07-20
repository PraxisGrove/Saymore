use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use slint::ComponentHandle;
use template_app::{
    AudioRecorder, DictationCompletionError, DictationCompletionResult, DictationHandoff,
    DictationSession, DictationSessionId, FailedDictation, RecordingError,
};

use crate::{
    DictationOverlays, RecorderHandle, delivery_runtime,
    dictation_completion_runtime::{CompletionContext, DictationRuntime},
    hide_overlay_after_delay,
    refinement_runtime::ProcessingActivity,
    ui::{AppWindow, RecordingOverlay, ResultOverlay},
    ui_status::{apply_asr_error, apply_recording_error},
};

pub(crate) struct CompletionWorkerContext {
    pub(crate) ui: slint::Weak<AppWindow>,
    pub(crate) status_overlay: slint::Weak<RecordingOverlay>,
    pub(crate) result_overlay: slint::Weak<ResultOverlay>,
    pub(crate) overlay_generation: i32,
    pub(crate) session: Arc<DictationSession>,
    pub(crate) dictation: DictationRuntime,
    pub(crate) feedback_sounds_enabled: Arc<AtomicBool>,
    pub(crate) copy_to_clipboard: bool,
}

struct FinishWorkerContext {
    completion: CompletionWorkerContext,
    recorder: RecorderHandle,
    id: DictationSessionId,
}

pub fn finish_recording(
    ui: slint::Weak<AppWindow>,
    overlays: DictationOverlays,
    recorder: RecorderHandle,
    session: Arc<DictationSession>,
    id: DictationSessionId,
    dictation: DictationRuntime,
    feedback_sounds_enabled: Arc<AtomicBool>,
) {
    let _ = overlays.limit.upgrade_in_event_loop(|limit| {
        let _ = limit.hide();
    });
    let queue_failure_session = Arc::clone(&session);
    let queue_failure_asr = Arc::clone(&dictation.asr);
    let queue_failure_recorder = Arc::clone(&recorder);
    if ui
        .upgrade_in_event_loop(move |ui| {
            let processing_label = ProcessingActivity::Transcribing.localized_label(&ui);
            ui.set_recording_active(false);
            ui.set_recording_failed(false);
            ui.set_recording_complete(false);
            ui.set_recording_attempted(false);
            ui.set_recording_status(processing_label.clone());
            ui.set_recording_detail(processing_label.clone());
            let overlay_generation = overlays
                .status
                .upgrade()
                .map(|overlay| {
                    overlay.set_mode(1);
                    overlay.set_processing_label(processing_label);
                    overlay.get_session_generation()
                })
                .unwrap_or_default();
            let failure_session = Arc::clone(&session);
            let failure_asr = Arc::clone(&dictation.asr);
            let context = FinishWorkerContext {
                completion: CompletionWorkerContext {
                    ui: ui.as_weak(),
                    status_overlay: overlays.status,
                    result_overlay: overlays.result,
                    overlay_generation,
                    session,
                    dictation,
                    feedback_sounds_enabled,
                    copy_to_clipboard: ui.get_copy_to_clipboard(),
                },
                recorder,
                id,
            };
            if std::thread::Builder::new()
                .name("saymore-finish-dictation".to_owned())
                .spawn(move || finish_recording_worker(context))
                .is_err()
            {
                failure_session.complete();
                failure_asr.cancel();
                apply_recording_error(
                    &ui,
                    &RecordingError::Capture("failed to start transcription worker".to_owned()),
                );
            }
        })
        .is_err()
    {
        queue_failure_session.complete();
        queue_failure_asr.cancel();
        if let Ok(mut recorder) = queue_failure_recorder.lock() {
            let _ = recorder.stop();
        }
    }
}

pub(crate) fn finish_retained_recording(
    context: CompletionWorkerContext,
    handoff: DictationHandoff,
) {
    let failure_session = Arc::clone(&context.session);
    let failure_ui = context.ui.clone();
    let failure_overlay = context.status_overlay.clone();
    if std::thread::Builder::new()
        .name("saymore-undo-dictation".to_owned())
        .spawn(move || {
            let result = complete_handoff(&context, handoff);
            finish_worker(context, result);
        })
        .is_err()
    {
        failure_session.complete();
        let _ = failure_ui.upgrade_in_event_loop(move |ui| {
            apply_recording_error(
                &ui,
                &RecordingError::Capture("failed to start transcription worker".to_owned()),
            );
            hide_overlay_after_delay(failure_overlay);
        });
    }
}

fn finish_recording_worker(context: FinishWorkerContext) {
    let FinishWorkerContext {
        completion,
        recorder,
        id,
    } = context;
    let recording_result = match recorder.lock() {
        Ok(mut recorder) if recorder.is_recording() => recorder.stop(),
        Ok(_) => Err(RecordingError::NotRecording),
        Err(_) => Err(RecordingError::Capture(
            "recorder lock was poisoned".to_owned(),
        )),
    };
    let result = match recording_result {
        Ok(recording) => match completion.dictation.asr.take() {
            Ok(recognition) => complete_handoff(
                &completion,
                DictationHandoff::Captured {
                    id,
                    recording,
                    recognition,
                },
            ),
            Err(error) => DictationCompletionResult::Failed(FailedDictation {
                id,
                error: DictationCompletionError::Recognition(error),
            }),
        },
        Err(error) => complete_handoff(
            &completion,
            DictationHandoff::CaptureFailed {
                id,
                error,
                recognition: completion.dictation.asr.take().ok(),
            },
        ),
    };
    finish_worker(completion, result);
}

fn complete_handoff(
    context: &CompletionWorkerContext,
    handoff: DictationHandoff,
) -> DictationCompletionResult {
    context.dictation.complete(
        handoff,
        CompletionContext {
            ui: context.ui.clone(),
            status_overlay: context.status_overlay.clone(),
            overlay_generation: context.overlay_generation,
            copy_to_clipboard: context.copy_to_clipboard,
        },
    )
}

fn finish_worker(context: CompletionWorkerContext, result: DictationCompletionResult) {
    context.session.complete();
    let ui = context.ui.clone();
    let _ = ui.upgrade_in_event_loop(move |ui| {
        apply_completion_result(&ui, result, context);
    });
}

fn apply_completion_result(
    ui: &AppWindow,
    result: DictationCompletionResult,
    context: CompletionWorkerContext,
) {
    match result {
        DictationCompletionResult::Completed(completed) => {
            crate::settings_ui::mark_asr_runtime_healthy(ui);
            delivery_runtime::present_completion(
                ui,
                context.status_overlay,
                context.overlay_generation,
                context.result_overlay,
                completed,
                context.feedback_sounds_enabled.load(Ordering::Acquire),
            );
        }
        DictationCompletionResult::Failed(FailedDictation { id, error }) => {
            match completion_failure_feedback(&error) {
                CompletionFailureFeedback::Recording(error) => {
                    tracing::warn!(
                        target: "saymore::diagnostics",
                        event = "asr.finalization_failed",
                        dictation_id = %id,
                        stage = "recording",
                        reason = %error
                    );
                    apply_recording_failure(ui, context.status_overlay, error);
                }
                CompletionFailureFeedback::Recognition(error) => {
                    tracing::warn!(
                        target: "saymore::diagnostics",
                        event = "asr.finalization_failed",
                        dictation_id = %id,
                        stage = "recognition",
                        reason = %error
                    );
                    apply_asr_error(ui, error);
                    hide_overlay_after_delay(context.status_overlay);
                }
            }
        }
    }
}

enum CompletionFailureFeedback<'a> {
    Recording(&'a RecordingError),
    Recognition(&'a template_app::SpeechRecognitionError),
}

fn completion_failure_feedback(error: &DictationCompletionError) -> CompletionFailureFeedback<'_> {
    match error {
        DictationCompletionError::Recording(error) => CompletionFailureFeedback::Recording(error),
        DictationCompletionError::Recognition(error) => {
            CompletionFailureFeedback::Recognition(error)
        }
    }
}

fn apply_recording_failure(
    ui: &AppWindow,
    status_overlay: slint::Weak<RecordingOverlay>,
    error: &RecordingError,
) {
    apply_recording_error(ui, error);
    hide_overlay_after_delay(status_overlay);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inactive_recorder_finishes_with_visible_failure_feedback() {
        let error = DictationCompletionError::Recording(RecordingError::NotRecording);

        assert!(matches!(
            completion_failure_feedback(&error),
            CompletionFailureFeedback::Recording(RecordingError::NotRecording)
        ));
    }
}
