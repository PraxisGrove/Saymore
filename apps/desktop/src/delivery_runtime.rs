use std::{
    sync::Arc,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use slint::{ComponentHandle, SharedString, Timer};
use template_app::{
    CorrectionObservingTextDeliverer, DeliveryTargetPrivacy, DictionaryLearningOutcome,
    DictionaryLearningStore, FeedbackSound, HistoryDelivery, HistoryStore,
    NewDictionaryObservation, NewHistoryRecord, PcmRecording, ProcessedText, RefinementStatus,
    TextDeliverer, TextDeliveryError, TextDeliveryOutcome, correction_from_edit,
};
use template_infra::{MacOsTextDeliverer, SqliteStorage, copy_text_to_clipboard};
use uuid::Uuid;

use crate::{
    overlay_generation_matches, overlay_window, play_feedback_sound,
    ui::{AppWindow, RecordingOverlay, ResultOverlay, Translations},
    ui_status::{apply_transcription_completed, delivery_requires_copy_recovery},
};

const FOCUS_SETTLE_DELAY: Duration = Duration::from_millis(80);
const FALLBACK_NOTICE_DELAY: Duration = Duration::from_millis(900);
pub fn wire_result_actions(overlay: &ResultOverlay) {
    let copy_overlay = overlay.as_weak();
    overlay.on_copy_result(move || {
        let Some(overlay) = copy_overlay.upgrade() else {
            return;
        };
        if copy_text_to_clipboard(overlay.get_transcript().as_str()).is_ok() {
            let _ = overlay.hide();
        }
    });

    let close_overlay = overlay.as_weak();
    overlay.on_close_result(move || {
        if let Some(overlay) = close_overlay.upgrade() {
            let _ = overlay.hide();
        }
    });
}

pub(crate) struct DeliveryRequest {
    pub ui: slint::Weak<AppWindow>,
    pub status_overlay: slint::Weak<RecordingOverlay>,
    pub overlay_generation: i32,
    pub result_overlay: slint::Weak<ResultOverlay>,
    pub recording: PcmRecording,
    pub processed: ProcessedText,
    pub history: Option<NewHistoryRecord>,
    pub storage: Arc<SqliteStorage>,
}

#[derive(Clone)]
struct ReadyDeliveryRequest {
    ui: slint::Weak<AppWindow>,
    status_overlay: slint::Weak<RecordingOverlay>,
    overlay_generation: i32,
    result_overlay: slint::Weak<ResultOverlay>,
    recording: PcmRecording,
    processed: ProcessedText,
    history_id: Option<String>,
    storage: Arc<SqliteStorage>,
}

impl DeliveryRequest {
    fn into_ready(self, history_id: Option<String>) -> ReadyDeliveryRequest {
        ReadyDeliveryRequest {
            ui: self.ui,
            status_overlay: self.status_overlay,
            overlay_generation: self.overlay_generation,
            result_overlay: self.result_overlay,
            recording: self.recording,
            processed: self.processed,
            history_id,
            storage: self.storage,
        }
    }
}

pub fn schedule_delivery(mut request: DeliveryRequest) {
    if !history_allowed_for_target(MacOsTextDeliverer.target_privacy()) {
        request.history = None;
    }
    let Some(record) = request.history.take() else {
        schedule_ready_delivery(request.into_ready(None));
        return;
    };

    let history_id = record.id.clone();
    let ready = request.into_ready(None);
    let fallback = ready.clone();
    let event_ui = ready.ui.clone();
    let storage = Arc::clone(&ready.storage);
    let spawn = thread::Builder::new()
        .name("saymore-create-history".to_owned())
        .spawn(move || {
            let result = storage.insert_history(record);
            let mut ready = ready;
            if result.is_ok() {
                ready.history_id = Some(history_id);
            }
            let _ = event_ui.upgrade_in_event_loop(move |ui| {
                match result {
                    Ok(()) => ui.invoke_refresh_usage(),
                    Err(error) => {
                        tracing::warn!(event = "history.create_failed", reason = %error);
                        ui.set_history_status(ui.global::<Translations>().get_storage_error());
                    }
                }
                schedule_ready_delivery(ready);
            });
        });
    if spawn.is_err() {
        if let Some(ui) = fallback.ui.upgrade() {
            ui.set_history_status(ui.global::<Translations>().get_storage_error());
        }
        schedule_ready_delivery(fallback);
    }
}

fn history_allowed_for_target(privacy: DeliveryTargetPrivacy) -> bool {
    privacy == DeliveryTargetPrivacy::Standard
}

fn schedule_ready_delivery(request: ReadyDeliveryRequest) {
    let fallback = match &request.processed.refinement {
        RefinementStatus::FellBack(reason) => Some(reason),
        RefinementStatus::Disabled | RefinementStatus::Skipped(_) | RefinementStatus::Completed => {
            None
        }
    };
    if let Some(overlay) = request.status_overlay.upgrade()
        && overlay_generation_matches(request.overlay_generation, overlay.get_session_generation())
    {
        if fallback.is_some() {
            overlay.set_mode(3);
        } else {
            overlay.set_mode(1);
        }
    }

    let notice_delay = if fallback.is_some() {
        FALLBACK_NOTICE_DELAY
    } else {
        Duration::ZERO
    };
    Timer::single_shot(notice_delay, move || {
        if let Some(overlay) = request.status_overlay.upgrade()
            && overlay_generation_matches(
                request.overlay_generation,
                overlay.get_session_generation(),
            )
        {
            let _ = overlay.hide();
        }
        Timer::single_shot(FOCUS_SETTLE_DELAY, move || complete_delivery(request));
    });
}

fn complete_delivery(pending: ReadyDeliveryRequest) {
    let learning_storage = Arc::clone(&pending.storage);
    let learning_ui = pending.ui.clone();
    let dictation_id = Uuid::new_v4().to_string();
    let delivery = MacOsTextDeliverer.deliver_and_observe(
        &pending.processed.text,
        Box::new(move |edit| {
            let Some(correction) = correction_from_edit(&edit.original, &edit.edited) else {
                return;
            };
            let result = learning_storage.record_dictionary_observation(NewDictionaryObservation {
                dictation_id,
                language: "und".to_owned(),
                correction,
                observed_at_ms: now_ms(),
            });
            let _ = learning_ui.upgrade_in_event_loop(move |ui| match result {
                Ok(DictionaryLearningOutcome::Added(entry)) => {
                    ui.set_dictionary_status(
                        ui.global::<Translations>()
                            .invoke_dictionary_automatically_added(entry.canonical.into()),
                    );
                    ui.invoke_refresh_dictionary();
                }
                Ok(
                    DictionaryLearningOutcome::Pending { .. }
                    | DictionaryLearningOutcome::Suppressed,
                ) => {}
                Err(error) => {
                    tracing::warn!(event = "dictionary.learning_failed", reason = %error);
                }
            });
        }),
    );
    let requires_recovery = delivery_requires_copy_recovery(&delivery);
    tracing::info!(
        target: "saymore::diagnostics",
        event = "delivery.completed",
        result = ?delivery,
        requires_recovery
    );
    let history_action = history_delivery_action(&delivery);
    if delivery.is_ok() {
        play_feedback_sound(FeedbackSound::Finish);
    }
    if let Some(overlay) = pending.status_overlay.upgrade()
        && overlay_generation_matches(pending.overlay_generation, overlay.get_session_generation())
    {
        let _ = overlay.hide();
    }
    if let Some(ui) = pending.ui.upgrade() {
        apply_transcription_completed(&ui, &pending.recording, &pending.processed, delivery);
    }
    if requires_recovery && let Some(overlay) = pending.result_overlay.upgrade() {
        show_result_overlay(&overlay, &pending.processed.text);
    }
    if let Some(history_id) = pending.history_id {
        persist_delivery_outcome(pending.ui, pending.storage, history_id, history_action);
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HistoryDeliveryAction {
    DiscardSensitive,
    MarkDelivered,
    KeepPending,
}

fn history_delivery_action(
    delivery: &Result<TextDeliveryOutcome, TextDeliveryError>,
) -> HistoryDeliveryAction {
    match delivery {
        Ok(TextDeliveryOutcome::SecureClipboardAttempted) => {
            HistoryDeliveryAction::DiscardSensitive
        }
        Ok(TextDeliveryOutcome::AccessibilityVerified | TextDeliveryOutcome::ClipboardVerified) => {
            HistoryDeliveryAction::MarkDelivered
        }
        Err(TextDeliveryError::SecureDeliveryFailed(_)) => HistoryDeliveryAction::DiscardSensitive,
        Ok(TextDeliveryOutcome::ClipboardAttempted) | Err(_) => HistoryDeliveryAction::KeepPending,
    }
}

fn persist_delivery_outcome(
    ui: slint::Weak<AppWindow>,
    storage: Arc<SqliteStorage>,
    history_id: String,
    action: HistoryDeliveryAction,
) {
    if action == HistoryDeliveryAction::KeepPending {
        if let Some(ui) = ui.upgrade() {
            ui.invoke_refresh_history();
        }
        return;
    }
    let refresh_ui = ui.clone();
    let failure_ui = ui;
    let refresh_usage = action == HistoryDeliveryAction::DiscardSensitive;
    let spawn = thread::Builder::new()
        .name("saymore-update-history".to_owned())
        .spawn(move || {
            let result = match action {
                HistoryDeliveryAction::DiscardSensitive => storage.delete_history(&history_id),
                HistoryDeliveryAction::MarkDelivered => {
                    storage.update_history_delivery(&history_id, HistoryDelivery::Delivered)
                }
                HistoryDeliveryAction::KeepPending => Ok(()),
            };
            let _ = refresh_ui.upgrade_in_event_loop(move |ui| {
                match result {
                    Ok(()) if refresh_usage => ui.invoke_refresh_usage(),
                    Ok(()) => {}
                    Err(error) => {
                        tracing::warn!(event = "history.delivery_update_failed", reason = %error);
                        ui.set_history_status(ui.global::<Translations>().get_storage_error());
                    }
                }
                ui.invoke_refresh_history();
            });
        });
    if spawn.is_err()
        && let Some(ui) = failure_ui.upgrade()
    {
        ui.set_history_status(ui.global::<Translations>().get_storage_error());
    }
}

fn show_result_overlay(overlay: &ResultOverlay, transcript: &str) {
    overlay.set_transcript(SharedString::from(transcript));
    if let Err(error) = overlay_window::present(overlay.window()) {
        tracing::warn!(event = "delivery.recovery_present_failed", reason = %error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secure_delivery_is_discarded_while_verified_delivery_is_marked_delivered() {
        assert_eq!(
            [
                HistoryDeliveryAction::DiscardSensitive,
                HistoryDeliveryAction::DiscardSensitive,
                HistoryDeliveryAction::MarkDelivered,
                HistoryDeliveryAction::KeepPending,
                HistoryDeliveryAction::KeepPending,
            ],
            [
                history_delivery_action(&Ok(TextDeliveryOutcome::SecureClipboardAttempted)),
                history_delivery_action(&Err(TextDeliveryError::SecureDeliveryFailed(
                    "paste event failed".to_owned(),
                ))),
                history_delivery_action(&Ok(TextDeliveryOutcome::ClipboardVerified)),
                history_delivery_action(&Ok(TextDeliveryOutcome::ClipboardAttempted)),
                history_delivery_action(&Err(TextDeliveryError::NoFocusedControl)),
            ]
        );
    }

    #[test]
    fn secure_target_preflight_prevents_history_creation() {
        assert!(history_allowed_for_target(DeliveryTargetPrivacy::Standard));
        assert!(!history_allowed_for_target(
            DeliveryTargetPrivacy::Sensitive
        ));
    }
}
