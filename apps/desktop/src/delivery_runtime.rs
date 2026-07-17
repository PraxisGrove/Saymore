use std::{
    sync::Arc,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use slint::{ComponentHandle, SharedString, Timer};
use template_app::{
    CorrectionObservingTextDeliverer, DictationSessionId, DictionaryLearningOutcome,
    DictionaryLearningStore, FeedbackSound, HistoryDelivery, HistoryStore,
    NewDictionaryObservation, NewHistoryRecord, PcmRecording, ProcessedText, RefinementStatus,
    TextDeliveryError, TextDeliveryOutcome, correction_from_edit,
};
use template_infra::{MacOsTextDeliverer, SqliteStorage, copy_text_to_clipboard};

use crate::{
    overlay_generation_matches, overlay_window, play_feedback_sound,
    refinement_runtime::RefinementRuntime,
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
    pub id: DictationSessionId,
    pub ui: slint::Weak<AppWindow>,
    pub status_overlay: slint::Weak<RecordingOverlay>,
    pub overlay_generation: i32,
    pub result_overlay: slint::Weak<ResultOverlay>,
    pub recording: PcmRecording,
    pub processed: ProcessedText,
    pub history: Option<NewHistoryRecord>,
    pub storage: Arc<SqliteStorage>,
    pub refinement: Arc<RefinementRuntime>,
    pub feedback_sounds_enabled: bool,
    pub copy_to_clipboard: bool,
}

#[derive(Clone)]
struct ReadyDeliveryRequest {
    id: DictationSessionId,
    ui: slint::Weak<AppWindow>,
    status_overlay: slint::Weak<RecordingOverlay>,
    overlay_generation: i32,
    result_overlay: slint::Weak<ResultOverlay>,
    recording: PcmRecording,
    processed: ProcessedText,
    history: Option<NewHistoryRecord>,
    storage: Arc<SqliteStorage>,
    refinement: Arc<RefinementRuntime>,
    feedback_sounds_enabled: bool,
    copy_to_clipboard: bool,
}

impl DeliveryRequest {
    fn into_ready(self) -> ReadyDeliveryRequest {
        ReadyDeliveryRequest {
            id: self.id,
            ui: self.ui,
            status_overlay: self.status_overlay,
            overlay_generation: self.overlay_generation,
            result_overlay: self.result_overlay,
            recording: self.recording,
            processed: self.processed,
            history: self.history,
            storage: self.storage,
            refinement: self.refinement,
            feedback_sounds_enabled: self.feedback_sounds_enabled,
            copy_to_clipboard: self.copy_to_clipboard,
        }
    }
}

pub fn schedule_delivery(request: DeliveryRequest) {
    schedule_ready_delivery(request.into_ready());
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
    let learning_refinement = Arc::clone(&pending.refinement);
    let dictation_id = pending.id.to_string();
    let delivery = MacOsTextDeliverer.deliver_and_observe(
        &pending.processed.text,
        Box::new(move |edit| {
            let Some(correction) = correction_from_edit(&edit.original, &edit.edited) else {
                return;
            };
            let language = inferred_dictionary_language(&correction.canonical).to_owned();
            let assessment = learning_refinement.assess_dictionary_correction(
                &correction.canonical,
                &edit.original,
                &edit.edited,
                &language,
            );
            let result = learning_storage.record_dictionary_observation(NewDictionaryObservation {
                dictation_id,
                language,
                correction,
                assessment,
                observed_at_ms: now_ms(),
            });
            let _ = learning_ui.upgrade_in_event_loop(move |ui| match result {
                Ok(DictionaryLearningOutcome::Added(entry)) => {
                    ui.set_dictionary_status(
                        ui.global::<Translations>()
                            .invoke_dictionary_automatically_added(entry.canonical.clone().into()),
                    );
                    ui.invoke_refresh_dictionary();
                    ui.invoke_show_dictionary_added(entry.id.into(), entry.canonical.into());
                }
                Ok(DictionaryLearningOutcome::Pending { .. }) => {
                    ui.invoke_refresh_dictionary();
                }
                Ok(DictionaryLearningOutcome::Rejected | DictionaryLearningOutcome::Suppressed) => {
                }
                Err(error) => {
                    tracing::warn!(event = "dictionary.learning_failed", reason = %error);
                }
            });
        }),
    );
    let requires_recovery = delivery_requires_copy_recovery(&delivery);
    if should_preserve_clipboard(pending.copy_to_clipboard, &delivery)
        && let Err(error) = copy_text_to_clipboard(&pending.processed.text)
    {
        tracing::warn!(event = "delivery.clipboard_copy_failed", reason = %error);
    }
    tracing::info!(
        target: "saymore::diagnostics",
        event = "delivery.completed",
        dictation_id = %pending.id,
        result = ?delivery,
        requires_recovery
    );
    let history_action = history_delivery_action(&delivery);
    let history = history_record_after_delivery(pending.history, history_action);
    if delivery.is_ok() && pending.feedback_sounds_enabled {
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
    if let Some(history) = history {
        persist_history(pending.ui, pending.storage, history);
    }
}

fn inferred_dictionary_language(text: &str) -> &'static str {
    if text.chars().any(
        |character| matches!(character as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF),
    ) {
        "zh-Hans"
    } else {
        "en"
    }
}

fn should_preserve_clipboard(
    enabled: bool,
    delivery: &Result<TextDeliveryOutcome, TextDeliveryError>,
) -> bool {
    enabled
        && !matches!(
            delivery,
            Ok(TextDeliveryOutcome::SecureClipboardAttempted)
                | Err(TextDeliveryError::SecureDeliveryFailed(_))
        )
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

fn history_record_after_delivery(
    record: Option<NewHistoryRecord>,
    action: HistoryDeliveryAction,
) -> Option<NewHistoryRecord> {
    let mut record = record?;
    match action {
        HistoryDeliveryAction::DiscardSensitive => None,
        HistoryDeliveryAction::MarkDelivered => {
            record.delivery = HistoryDelivery::Delivered;
            Some(record)
        }
        HistoryDeliveryAction::KeepPending => {
            record.delivery = HistoryDelivery::NotDelivered;
            Some(record)
        }
    }
}

fn persist_history(
    ui: slint::Weak<AppWindow>,
    storage: Arc<SqliteStorage>,
    record: NewHistoryRecord,
) {
    let refresh_ui = ui.clone();
    let failure_ui = ui;
    let spawn = thread::Builder::new()
        .name("saymore-create-history".to_owned())
        .spawn(move || {
            let result = storage.insert_history(record);
            let _ = refresh_ui.upgrade_in_event_loop(move |ui| {
                match result {
                    Ok(()) => ui.invoke_refresh_usage(),
                    Err(error) => {
                        tracing::warn!(event = "history.create_failed", reason = %error);
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
    fn optional_clipboard_copy_excludes_secure_input() {
        assert!(should_preserve_clipboard(
            true,
            &Ok(TextDeliveryOutcome::AccessibilityVerified)
        ));
        assert!(!should_preserve_clipboard(
            false,
            &Ok(TextDeliveryOutcome::AccessibilityVerified)
        ));
        assert!(!should_preserve_clipboard(
            true,
            &Ok(TextDeliveryOutcome::SecureClipboardAttempted)
        ));
        assert!(!should_preserve_clipboard(
            true,
            &Err(TextDeliveryError::SecureDeliveryFailed(
                "secure input".to_owned()
            ))
        ));
    }

    #[test]
    fn history_is_created_only_after_the_final_delivery_classification() {
        let record = NewHistoryRecord {
            id: "history-1".to_owned(),
            created_at_ms: 1,
            final_text: "hello".to_owned(),
            raw_asr_text: None,
            llm_refined_text: None,
            audio_duration_ms: 10,
            language: None,
            delivery: HistoryDelivery::NotDelivered,
            refinement: template_app::HistoryRefinement::NotUsed,
            asr_provider_id: None,
            llm_provider_id: None,
            asr_model: None,
            llm_model: None,
        };

        assert_eq!(
            None,
            history_record_after_delivery(
                Some(record.clone()),
                HistoryDeliveryAction::DiscardSensitive
            )
        );
        assert_eq!(
            Some(HistoryDelivery::Delivered),
            history_record_after_delivery(
                Some(record.clone()),
                HistoryDeliveryAction::MarkDelivered
            )
            .map(|record| record.delivery)
        );
        assert_eq!(
            Some(HistoryDelivery::NotDelivered),
            history_record_after_delivery(Some(record), HistoryDeliveryAction::KeepPending)
                .map(|record| record.delivery)
        );
    }
}
