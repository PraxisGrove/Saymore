use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Instant;

use slint::{ComponentHandle, SharedString};
use template_app::{AudioRecorder, FeedbackSound, RecordingError, SpeechRecognitionError};
use template_infra::MacOsAudioRecorder;

use crate::refinement_runtime::ProcessingActivity;
use crate::{
    DictationOverlays, TextProcessingServices, delivery_runtime, hide_overlay_after_delay,
    play_feedback_sound,
    ui::AppWindow,
    ui_status::{apply_asr_error, apply_recording_error},
};

enum FinishError {
    Recording(RecordingError),
    Recognition(SpeechRecognitionError),
}

pub fn finish_recording(
    ui: slint::Weak<AppWindow>,
    overlays: DictationOverlays,
    recorder: Arc<Mutex<MacOsAudioRecorder>>,
    recording_active: Arc<AtomicBool>,
    processing: TextProcessingServices,
) {
    let plan = processing.refinement.plan();
    let processing_label = ProcessingActivity::Transcribing.label();
    let processing_overlay = overlays.status.clone();
    let _ = ui.upgrade_in_event_loop(move |ui| {
        ui.set_recording_active(false);
        ui.set_recording_failed(false);
        ui.set_recording_complete(false);
        ui.set_recording_status(SharedString::from(processing_label));
        ui.set_recording_detail(SharedString::from(processing_label));
        if let Some(overlay) = processing_overlay.upgrade() {
            overlay.set_mode(1);
            overlay.set_show_device(false);
            overlay.set_processing_label(SharedString::from(processing_label));
        }
    });
    let failure_ui = ui.clone();
    let failure_recording_active = Arc::clone(&recording_active);
    if std::thread::Builder::new()
        .name("saymore-finish-dictation".to_owned())
        .spawn(move || {
            finish_recording_worker(ui, overlays, recorder, recording_active, processing, plan);
        })
        .is_err()
    {
        failure_recording_active.store(false, Ordering::Relaxed);
        let _ = failure_ui.upgrade_in_event_loop(|ui| {
            apply_recording_error(
                &ui,
                &RecordingError::Capture("failed to start transcription worker".to_owned()),
            );
        });
    }
}

fn finish_recording_worker(
    ui: slint::Weak<AppWindow>,
    overlays: DictationOverlays,
    recorder: Arc<Mutex<MacOsAudioRecorder>>,
    recording_active: Arc<AtomicBool>,
    processing: TextProcessingServices,
    plan: crate::refinement_runtime::RefinementPlan,
) {
    let mut recorder = match recorder.lock() {
        Ok(recorder) if recorder.is_recording() => recorder,
        Ok(_) => return,
        Err(_) => {
            recording_active.store(false, Ordering::Relaxed);
            let _ = ui.upgrade_in_event_loop(|ui| {
                apply_recording_error(
                    &ui,
                    &RecordingError::Capture("recorder lock was poisoned".to_owned()),
                );
            });
            return;
        }
    };

    let recording_result = recorder.stop();
    let transcription_result = match recording_result {
        Ok(recording) => {
            let started = Instant::now();
            let result = processing
                .asr
                .finish()
                .map(|text| (recording, text))
                .map_err(FinishError::Recognition);
            log_asr_finalization(&result, started.elapsed().as_millis());
            result
        }
        Err(error) => {
            processing.asr.cancel();
            Err(FinishError::Recording(error))
        }
    };
    let processing_result = transcription_result.and_then(|(recording, transcript)| {
        let activity_ui = ui.clone();
        let activity_overlay = overlays.status.clone();
        processing
            .refinement
            .process_final_transcript(&transcript, plan, move || {
                show_processing_activity(
                    &activity_ui,
                    &activity_overlay,
                    ProcessingActivity::Refining,
                );
            })
            .map(|processed| (recording, processed))
            .map_err(FinishError::Recognition)
    });
    recording_active.store(false, Ordering::Relaxed);
    let _ = ui.upgrade_in_event_loop(move |ui| match processing_result {
        Ok((recording, processed)) => {
            delivery_runtime::schedule_delivery(
                ui.as_weak(),
                overlays.status,
                overlays.result,
                recording,
                processed,
            );
        }
        Err(FinishError::Recording(RecordingError::NotRecording)) => {}
        Err(FinishError::Recording(error)) => {
            play_feedback_sound(FeedbackSound::Failure);
            apply_recording_error(&ui, &error);
            hide_overlay_after_delay(overlays.status);
        }
        Err(FinishError::Recognition(error)) => {
            play_feedback_sound(FeedbackSound::Failure);
            apply_asr_error(&ui, &error);
            hide_overlay_after_delay(overlays.status);
        }
    });
}

fn log_asr_finalization<T>(result: &Result<T, FinishError>, duration_ms: u128) {
    match result {
        Ok(_) => tracing::info!(
            target: "saymore::diagnostics",
            event = "asr.finalized",
            duration_ms
        ),
        Err(_) => tracing::warn!(
            target: "saymore::diagnostics",
            event = "asr.finalization_failed",
            duration_ms
        ),
    }
}

pub(crate) fn show_processing_activity(
    ui: &slint::Weak<AppWindow>,
    overlay: &slint::Weak<crate::ui::RecordingOverlay>,
    activity: ProcessingActivity,
) {
    match activity {
        ProcessingActivity::Transcribing => return,
        ProcessingActivity::Refining => {}
    }
    let label = activity.label();
    let overlay = overlay.clone();
    let _ = ui.upgrade_in_event_loop(move |ui| {
        ui.set_recording_status(SharedString::from(label));
        ui.set_recording_detail(SharedString::from(label));
        if let Some(overlay) = overlay.upgrade() {
            overlay.set_processing_label(SharedString::from(label));
        }
    });
}
