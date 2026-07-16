use std::sync::{Arc, Mutex, atomic::Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use slint::ComponentHandle;
use template_app::{
    AudioRecorder, DictationSession, HistoryDelivery, HistoryRefinement, LocalSettingsStore,
    NewHistoryRecord, ProviderConfigStore, RecordingError, RefinementFallbackReason,
    RefinementStatus, SpeechRecognitionError,
};
use template_infra::MacOsAudioRecorder;
use uuid::Uuid;

use crate::refinement_runtime::ProcessingActivity;
use crate::{
    DictationOverlays, TextProcessingServices, delivery_runtime, hide_overlay_after_delay,
    ui::{AppWindow, Translations},
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
    session: Arc<DictationSession>,
    processing: TextProcessingServices,
) {
    let _ = overlays.limit.upgrade_in_event_loop(|limit| {
        let _ = limit.hide();
    });
    let plan = processing.refinement.plan();
    let queue_failure_session = Arc::clone(&session);
    let queue_failure_asr = Arc::clone(&processing.asr);
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
            let worker_ui = ui.as_weak();
            let failure_session = Arc::clone(&session);
            if std::thread::Builder::new()
                .name("saymore-finish-dictation".to_owned())
                .spawn(move || {
                    finish_recording_worker(
                        worker_ui,
                        overlays,
                        recorder,
                        session,
                        processing,
                        plan,
                        overlay_generation,
                    );
                })
                .is_err()
            {
                failure_session.complete();
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

fn finish_recording_worker(
    ui: slint::Weak<AppWindow>,
    overlays: DictationOverlays,
    recorder: Arc<Mutex<MacOsAudioRecorder>>,
    session: Arc<DictationSession>,
    processing: TextProcessingServices,
    plan: crate::refinement_runtime::RefinementPlan,
    overlay_generation: i32,
) {
    let mut recorder = match recorder.lock() {
        Ok(recorder) if recorder.is_recording() => recorder,
        Ok(_) => {
            session.complete();
            return;
        }
        Err(_) => {
            session.complete();
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
        let relevant_terms =
            crate::refinement_runtime::dictionary_terms_for_current_refinement(&processing.storage);
        processing
            .refinement
            .process_final_transcript(&transcript, plan, relevant_terms, move || {
                show_processing_activity(
                    &activity_ui,
                    &activity_overlay,
                    ProcessingActivity::Refining,
                );
            })
            .map(|outcome| {
                let history = prepare_history(
                    &processing,
                    &recording,
                    &transcript,
                    outcome.llm_refined_text.as_deref(),
                    &outcome.processed,
                );
                (recording, outcome.processed, history)
            })
            .map_err(FinishError::Recognition)
    });
    session.complete();
    let _ = ui.upgrade_in_event_loop(move |ui| match processing_result {
        Ok((recording, processed, history)) => {
            crate::settings_ui::mark_asr_runtime_healthy(&ui);
            if let Some(error) = &history.error {
                tracing::warn!(event = "history.prepare_failed", reason = %error);
                ui.set_history_status(ui.global::<Translations>().get_storage_error());
            }
            delivery_runtime::schedule_delivery(delivery_runtime::DeliveryRequest {
                ui: ui.as_weak(),
                status_overlay: overlays.status,
                overlay_generation,
                result_overlay: overlays.result,
                recording,
                processed,
                history: history.record,
                storage: processing.storage,
                refinement: processing.refinement,
                feedback_sounds_enabled: processing.feedback_sounds_enabled.load(Ordering::Acquire),
                copy_to_clipboard: ui.get_copy_to_clipboard(),
            });
        }
        Err(FinishError::Recording(RecordingError::NotRecording)) => {}
        Err(FinishError::Recording(error)) => {
            apply_recording_error(&ui, &error);
            hide_overlay_after_delay(overlays.status);
        }
        Err(FinishError::Recognition(error)) => {
            apply_asr_error(&ui, &error);
            hide_overlay_after_delay(overlays.status);
        }
    });
}

pub(crate) struct PreparedHistory {
    pub(crate) record: Option<NewHistoryRecord>,
    pub(crate) error: Option<String>,
}

pub(crate) fn prepare_history(
    processing: &TextProcessingServices,
    recording: &template_app::PcmRecording,
    raw_asr_text: &str,
    llm_refined_text: Option<&str>,
    processed: &template_app::ProcessedText,
) -> PreparedHistory {
    let id = Uuid::new_v4().to_string();
    let settings = match processing.storage.load_settings() {
        Ok(settings) if settings.history_enabled => settings,
        Ok(_) => {
            return PreparedHistory {
                record: None,
                error: None,
            };
        }
        Err(error) => {
            return PreparedHistory {
                record: None,
                error: Some(error.to_string()),
            };
        }
    };
    let _ = settings;
    let catalog = processing.provider_config.load_catalog().ok();
    let asr_provider_id = catalog
        .as_ref()
        .and_then(|catalog| catalog.active.asr.clone());
    let llm_provider_id = catalog
        .as_ref()
        .and_then(|catalog| catalog.active.llm.clone());
    let asr_model = catalog.as_ref().and_then(|catalog| {
        catalog
            .asr_providers
            .iter()
            .find(|provider| Some(provider.id.as_str()) == asr_provider_id.as_deref())
            .and_then(|provider| provider.config.get("model"))
            .and_then(|value| value.as_str())
            .map(str::to_owned)
    });
    let llm_model = catalog.as_ref().and_then(|catalog| {
        catalog
            .llm_providers
            .iter()
            .find(|provider| Some(provider.id.as_str()) == llm_provider_id.as_deref())
            .and_then(|provider| provider.config.get("model"))
            .and_then(|value| value.as_str())
            .map(str::to_owned)
    });
    let record = NewHistoryRecord {
        id,
        created_at_ms: now_ms(),
        final_text: processed.text.clone(),
        raw_asr_text: experimental_asr_text(raw_asr_text),
        llm_refined_text: experimental_llm_refined_text(llm_refined_text),
        audio_duration_ms: recording.duration_ms,
        language: None,
        delivery: HistoryDelivery::NotDelivered,
        refinement: history_refinement(&processed.refinement),
        asr_provider_id,
        llm_provider_id,
        asr_model,
        llm_model,
    };
    PreparedHistory {
        record: Some(record),
        error: None,
    }
}

fn experimental_llm_refined_text(llm_refined_text: Option<&str>) -> Option<String> {
    #[cfg(any(debug_assertions, feature = "history-experiments"))]
    {
        llm_refined_text.map(str::to_owned)
    }
    #[cfg(not(any(debug_assertions, feature = "history-experiments")))]
    {
        let _ = llm_refined_text;
        None
    }
}

fn experimental_asr_text(raw_asr_text: &str) -> Option<String> {
    #[cfg(any(debug_assertions, feature = "history-experiments"))]
    {
        Some(raw_asr_text.to_owned())
    }
    #[cfg(not(any(debug_assertions, feature = "history-experiments")))]
    {
        let _ = raw_asr_text;
        None
    }
}

fn history_refinement(status: &RefinementStatus) -> HistoryRefinement {
    match status {
        RefinementStatus::Disabled | RefinementStatus::Skipped(_) => HistoryRefinement::NotUsed,
        RefinementStatus::Completed => HistoryRefinement::Completed,
        RefinementStatus::FellBack(RefinementFallbackReason::Timeout) => {
            HistoryRefinement::TimedOut
        }
        RefinementStatus::FellBack(RefinementFallbackReason::OutputRejected) => {
            HistoryRefinement::OutputRejected
        }
        RefinementStatus::FellBack(_) => HistoryRefinement::ProviderUnavailable,
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

fn log_asr_finalization<T>(result: &Result<T, FinishError>, duration_ms: u128) {
    match result {
        Ok(_) => tracing::info!(
            target: "saymore::diagnostics",
            event = "asr.finalized",
            duration_ms
        ),
        Err(FinishError::Recording(error)) => tracing::warn!(
            target: "saymore::diagnostics",
            event = "asr.finalization_failed",
            stage = "recording",
            reason = %error,
            duration_ms
        ),
        Err(FinishError::Recognition(error)) => tracing::warn!(
            target: "saymore::diagnostics",
            event = "asr.finalization_failed",
            stage = "recognition",
            reason = %error,
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
    let overlay = overlay.clone();
    let _ = ui.upgrade_in_event_loop(move |ui| {
        let label = activity.localized_label(&ui);
        ui.set_recording_status(label.clone());
        ui.set_recording_detail(label.clone());
        if let Some(overlay) = overlay.upgrade() {
            overlay.set_processing_label(label);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn experimental_history_fields_follow_the_build_contract() {
        let enabled = cfg!(any(debug_assertions, feature = "history-experiments"));

        assert_eq!(
            enabled.then(|| "ASR 原始结果".to_owned()),
            experimental_asr_text("ASR 原始结果")
        );
        assert_eq!(
            enabled.then(|| "LLM 润色结果".to_owned()),
            experimental_llm_refined_text(Some("LLM 润色结果"))
        );
        assert_eq!(None, experimental_llm_refined_text(None));
    }
}
