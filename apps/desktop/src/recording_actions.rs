use std::{
    sync::{Arc, Mutex},
    time::Instant,
};

use slint::{ComponentHandle, Timer};
use template_app::{AudioRecorder, CancelledRecordingStore, DictationSession, RecordingError};

use crate::{
    CANCEL_UNDO_WINDOW, DictationOverlays, RecorderHandle,
    dictation_completion_runtime::DictationRuntime,
    dictation_finish, overlay_generation_matches, refinement_runtime,
    ui::{AppWindow, RecordingLimitOverlay, RecordingOverlay, ResultOverlay, Translations},
    ui_status::apply_recording_error,
};

#[derive(Clone)]
pub(crate) struct RecordingActionRuntime {
    pub(crate) recorder: RecorderHandle,
    pub(crate) session: Arc<DictationSession>,
    pub(crate) cancelled: Arc<Mutex<CancelledRecordingStore>>,
    pub(crate) dictation: DictationRuntime,
    pub(crate) feedback_sounds_enabled: Arc<std::sync::atomic::AtomicBool>,
}

pub fn wire(
    ui: &AppWindow,
    overlay: &RecordingOverlay,
    result_overlay: &ResultOverlay,
    limit_overlay: &RecordingLimitOverlay,
    runtime: RecordingActionRuntime,
) -> impl Fn() + 'static {
    let finish_ui = ui.as_weak();
    let finish_overlays = DictationOverlays::new(overlay, result_overlay, limit_overlay);
    let finish_runtime = runtime.clone();
    overlay.on_finish(move || {
        if let Some(id) = finish_runtime.session.request_finish() {
            dictation_finish::finish_recording(
                finish_ui.clone(),
                finish_overlays.clone(),
                Arc::clone(&finish_runtime.recorder),
                Arc::clone(&finish_runtime.session),
                id,
                finish_runtime.dictation.clone(),
                Arc::clone(&finish_runtime.feedback_sounds_enabled),
            );
        }
    });
    let RecordingActionRuntime {
        recorder,
        session,
        cancelled,
        dictation,
        feedback_sounds_enabled,
    } = runtime;
    let pause_ui = ui.as_weak();
    let pause_overlay = overlay.as_weak();
    let pause_limit = limit_overlay.as_weak();
    let pause_recorder = Arc::clone(&recorder);
    let pause_session = Arc::clone(&session);
    let pause_cancelled = Arc::clone(&cancelled);
    let pause_asr = Arc::clone(&dictation.asr);
    let cancel_ui = ui.as_weak();
    let cancel_overlay = overlay.as_weak();
    let cancel_limit = limit_overlay.as_weak();
    let cancel_session = Arc::clone(&session);
    let cancel_store = Arc::clone(&cancelled);
    let cancel_asr = Arc::clone(&dictation.asr);
    overlay.on_cancel(move || {
        cancel(
            &cancel_ui,
            &cancel_overlay,
            &cancel_limit,
            &recorder,
            &cancel_session,
            &cancel_store,
            &cancel_asr,
        );
    });

    let undo_ui = ui.as_weak();
    let undo_overlay = overlay.as_weak();
    let undo_result_overlay = result_overlay.as_weak();
    let undo_dictation = dictation;
    let undo_feedback_sounds = feedback_sounds_enabled;
    let undo_session = Arc::clone(&session);
    overlay.on_undo_cancel(move || {
        undo_cancelled_recording(
            &undo_ui,
            &undo_overlay,
            undo_result_overlay.clone(),
            &cancelled,
            Arc::clone(&undo_session),
            undo_dictation.clone(),
            Arc::clone(&undo_feedback_sounds),
        );
    });

    move || {
        cancel(
            &pause_ui,
            &pause_overlay,
            &pause_limit,
            &pause_recorder,
            &pause_session,
            &pause_cancelled,
            &pause_asr,
        );
    }
}

pub fn cancel(
    ui: &slint::Weak<AppWindow>,
    overlay: &slint::Weak<RecordingOverlay>,
    limit_overlay: &slint::Weak<RecordingLimitOverlay>,
    recorder: &RecorderHandle,
    session: &DictationSession,
    cancelled: &Arc<Mutex<CancelledRecordingStore>>,
    asr: &crate::asr_runtime::AsrSessionController,
) {
    let Some(id) = session.request_cancel() else {
        return;
    };
    let _ = limit_overlay.upgrade_in_event_loop(|overlay| {
        let _ = overlay.hide();
    });
    asr.cancel();
    let result = recorder
        .lock()
        .map_err(|_| RecordingError::Capture("recorder lock was poisoned".to_owned()))
        .and_then(|mut recorder| recorder.stop());
    let cancel_overlay = overlay.clone();
    let cancelled = Arc::clone(cancelled);
    let _ = ui.upgrade_in_event_loop(move |ui| match result {
        Ok(recording) => {
            let generation = match cancelled.lock() {
                Ok(mut cancelled) => cancelled.retain(id, recording, Instant::now()),
                Err(_) => {
                    apply_recording_error(
                        &ui,
                        &RecordingError::Capture(
                            "cancelled recording lock was poisoned".to_owned(),
                        ),
                    );
                    return;
                }
            };
            ui.set_recording_active(false);
            ui.set_recording_complete(false);
            ui.set_recording_failed(false);
            ui.set_recording_attempted(false);
            ui.set_recording_level(0.0);
            let translations = ui.global::<Translations>();
            ui.set_recording_status(translations.get_recording_cancelled());
            ui.set_recording_detail(translations.get_recording_cancel_undo_hint());
            if let Some(overlay) = cancel_overlay.upgrade() {
                overlay.set_mode(2);
            }
            schedule_cancel_expiration(cancel_overlay, cancelled, generation);
        }
        Err(RecordingError::NotRecording) => {}
        Err(error) => apply_recording_error(&ui, &error),
    });
}

fn schedule_cancel_expiration(
    overlay: slint::Weak<RecordingOverlay>,
    cancelled: Arc<Mutex<CancelledRecordingStore>>,
    generation: u64,
) {
    let overlay_generation = overlay
        .upgrade()
        .map(|overlay| overlay.get_session_generation())
        .unwrap_or_default();
    Timer::single_shot(CANCEL_UNDO_WINDOW, move || {
        let expired = match cancelled.lock() {
            Ok(mut cancelled) => cancelled.expire(generation, Instant::now()),
            Err(_) => false,
        };
        if expired
            && let Some(overlay) = overlay.upgrade()
            && overlay_generation_matches(overlay_generation, overlay.get_session_generation())
        {
            crate::recording_runtime::animate_overlay_hide(&overlay, || {});
        }
    });
}

fn undo_cancelled_recording(
    ui: &slint::Weak<AppWindow>,
    overlay: &slint::Weak<RecordingOverlay>,
    result_overlay: slint::Weak<ResultOverlay>,
    cancelled: &Mutex<CancelledRecordingStore>,
    session: Arc<DictationSession>,
    dictation: DictationRuntime,
    feedback_sounds_enabled: Arc<std::sync::atomic::AtomicBool>,
) {
    let overlay_generation = overlay
        .upgrade()
        .map(|overlay| overlay.get_session_generation())
        .unwrap_or_default();
    let Some(id) = session.current_id() else {
        return;
    };
    if !session.begin_retained_processing(id) {
        return;
    }
    let handoff = cancelled
        .lock()
        .ok()
        .and_then(|mut cancelled| cancelled.take(Instant::now()));
    let Some(handoff) = handoff else {
        session.complete();
        return;
    };
    let mut copy_to_clipboard = false;
    if let Some(ui) = ui.upgrade() {
        copy_to_clipboard = ui.get_copy_to_clipboard();
        let processing_label =
            refinement_runtime::ProcessingActivity::Transcribing.localized_label(&ui);
        ui.set_recording_status(processing_label.clone());
        ui.set_recording_detail(processing_label.clone());
        if let Some(overlay) = overlay.upgrade() {
            overlay.set_mode(1);
            overlay.set_processing_label(processing_label);
        }
    }
    dictation_finish::finish_retained_recording(
        dictation_finish::CompletionWorkerContext {
            ui: ui.clone(),
            status_overlay: overlay.clone(),
            result_overlay,
            overlay_generation,
            session,
            dictation,
            feedback_sounds_enabled,
            copy_to_clipboard,
        },
        handoff,
    );
}
