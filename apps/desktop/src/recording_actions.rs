use std::{
    sync::{Arc, Mutex, atomic::Ordering},
    time::Instant,
};

use slint::{ComponentHandle, Timer};
use template_app::{AudioRecorder, CancelledRecordingStore, DictationSession, RecordingError};
use template_infra::MacOsAudioRecorder;

use crate::{
    CANCEL_UNDO_WINDOW, DictationOverlays, TextProcessingServices, delivery_runtime,
    dictation_finish, hide_overlay_after_delay, overlay_generation_matches, refinement_runtime,
    settings_ui,
    ui::{AppWindow, RecordingLimitOverlay, RecordingOverlay, ResultOverlay, Translations},
    ui_status::{apply_asr_error, apply_recording_error},
};

pub(crate) struct RecordingActionRuntime {
    pub(crate) recorder: Arc<Mutex<MacOsAudioRecorder>>,
    pub(crate) session: Arc<DictationSession>,
    pub(crate) cancelled: Arc<Mutex<CancelledRecordingStore>>,
    pub(crate) processing: TextProcessingServices,
}

pub fn wire(
    ui: &AppWindow,
    overlay: &RecordingOverlay,
    result_overlay: &ResultOverlay,
    limit_overlay: &RecordingLimitOverlay,
    runtime: RecordingActionRuntime,
) -> impl Fn() + 'static {
    let RecordingActionRuntime {
        recorder,
        session,
        cancelled,
        processing,
    } = runtime;
    let pause_ui = ui.as_weak();
    let pause_overlay = overlay.as_weak();
    let pause_limit = limit_overlay.as_weak();
    let pause_recorder = Arc::clone(&recorder);
    let pause_session = Arc::clone(&session);
    let pause_cancelled = Arc::clone(&cancelled);
    let pause_asr = Arc::clone(&processing.asr);
    let finish_ui = ui.as_weak();
    let finish_overlays = DictationOverlays::new(overlay, result_overlay, limit_overlay);
    let finish_recorder = Arc::clone(&recorder);
    let finish_session = Arc::clone(&session);
    let finish_processing = processing.clone();
    overlay.on_finish(move || {
        if finish_session.request_finish() {
            dictation_finish::finish_recording(
                finish_ui.clone(),
                finish_overlays.clone(),
                Arc::clone(&finish_recorder),
                Arc::clone(&finish_session),
                finish_processing.clone(),
            );
        }
    });

    let cancel_ui = ui.as_weak();
    let cancel_overlay = overlay.as_weak();
    let cancel_limit = limit_overlay.as_weak();
    let cancel_session = Arc::clone(&session);
    let cancel_store = Arc::clone(&cancelled);
    let cancel_asr = Arc::clone(&processing.asr);
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
    let undo_processing = processing;
    let undo_session = Arc::clone(&session);
    overlay.on_undo_cancel(move || {
        undo_cancelled_recording(
            &undo_ui,
            &undo_overlay,
            undo_result_overlay.clone(),
            &cancelled,
            Arc::clone(&undo_session),
            undo_processing.clone(),
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
    recorder: &Mutex<MacOsAudioRecorder>,
    session: &DictationSession,
    cancelled: &Arc<Mutex<CancelledRecordingStore>>,
    asr: &crate::asr_runtime::AsrSessionController,
) {
    if !session.request_cancel() {
        return;
    }
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
                Ok(mut cancelled) => cancelled.retain(recording, Instant::now()),
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
            let _ = overlay.hide();
        }
    });
}

fn undo_cancelled_recording(
    ui: &slint::Weak<AppWindow>,
    overlay: &slint::Weak<RecordingOverlay>,
    result_overlay: slint::Weak<ResultOverlay>,
    cancelled: &Mutex<CancelledRecordingStore>,
    session: Arc<DictationSession>,
    processing: TextProcessingServices,
) {
    let overlay_generation = overlay
        .upgrade()
        .map(|overlay| overlay.get_session_generation())
        .unwrap_or_default();
    if !session.begin_retained_processing() {
        return;
    }
    let recording = cancelled
        .lock()
        .ok()
        .and_then(|mut cancelled| cancelled.take(Instant::now()));
    let Some(recording) = recording else {
        session.complete();
        return;
    };
    let plan = processing.refinement.plan();
    if let Some(ui) = ui.upgrade() {
        let processing_label =
            refinement_runtime::ProcessingActivity::Transcribing.localized_label(&ui);
        ui.set_recording_status(processing_label.clone());
        ui.set_recording_detail(processing_label.clone());
        if let Some(overlay) = overlay.upgrade() {
            overlay.set_mode(1);
            overlay.set_processing_label(processing_label);
        }
    }
    let event_ui = ui.clone();
    let event_overlay = overlay.clone();
    let failure_ui = ui.clone();
    let failure_overlay = overlay.clone();
    let failure_session = Arc::clone(&session);
    let feedback_sounds_enabled = Arc::clone(&processing.feedback_sounds_enabled);
    let spawn_result = std::thread::Builder::new()
        .name("saymore-undo-dictation".to_owned())
        .spawn(move || {
            let result = processing.asr.start(Arc::new(|_| {})).and_then(|()| {
                for chunk in recording.samples.chunks(1_600) {
                    processing.asr.push_audio(chunk.to_vec())?;
                }
                processing.asr.finish()
            });
            let result = result.and_then(|transcript| {
                let activity_ui = event_ui.clone();
                let activity_overlay = event_overlay.clone();
                let relevant_terms = refinement_runtime::dictionary_terms_for_current_refinement(
                    &processing.storage,
                );
                processing
                    .refinement
                    .process_final_transcript(&transcript, plan, relevant_terms, move || {
                        dictation_finish::show_processing_activity(
                            &activity_ui,
                            &activity_overlay,
                            refinement_runtime::ProcessingActivity::Refining,
                        );
                    })
                    .map(|outcome| {
                        let history = dictation_finish::prepare_history(
                            &processing,
                            &recording,
                            &transcript,
                            outcome.llm_refined_text.as_deref(),
                            &outcome.processed,
                        );
                        (transcript, outcome.processed, history)
                    })
            });
            session.complete();
            let _ = event_ui.upgrade_in_event_loop(move |ui| match result {
                Ok((_transcript, processed, history)) => {
                    settings_ui::mark_asr_runtime_healthy(&ui);
                    if let Some(error) = history.error {
                        tracing::warn!(event = "history.prepare_failed", reason = %error);
                        ui.set_history_status(ui.global::<Translations>().get_storage_error());
                    }
                    delivery_runtime::schedule_delivery(delivery_runtime::DeliveryRequest {
                        ui: ui.as_weak(),
                        status_overlay: event_overlay,
                        overlay_generation,
                        result_overlay,
                        recording,
                        processed,
                        history: history.record,
                        storage: processing.storage,
                        refinement: processing.refinement,
                        feedback_sounds_enabled: feedback_sounds_enabled.load(Ordering::Acquire),
                        copy_to_clipboard: ui.get_copy_to_clipboard(),
                    });
                }
                Err(error) => {
                    apply_asr_error(&ui, &error);
                    hide_overlay_after_delay(event_overlay);
                }
            });
        });
    if spawn_result.is_err() {
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
