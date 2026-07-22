use slint::{ComponentHandle, SharedString};
use template_app::{CompletedDictation, DictationHistoryResult, FeedbackSound};
use template_infra::copy_text_to_clipboard;

use crate::{
    overlay_generation_matches, overlay_window, play_feedback_sound, recording_runtime,
    ui::{AppWindow, RecordingOverlay, ResultOverlay, Translations},
    ui_status::{apply_transcription_completed, delivery_requires_copy_recovery},
};

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

pub(crate) fn present_completion(
    ui: &AppWindow,
    status_overlay: slint::Weak<RecordingOverlay>,
    overlay_generation: i32,
    result_overlay: slint::Weak<ResultOverlay>,
    completed: CompletedDictation,
    feedback_sounds_enabled: bool,
) {
    let requires_recovery = delivery_requires_copy_recovery(&completed.delivery);
    match &completed.history {
        DictationHistoryResult::Saved(_) => {
            ui.invoke_refresh_usage();
            ui.invoke_refresh_history();
        }
        DictationHistoryResult::Failed { error, .. } => {
            tracing::warn!(
                target: "saymore::diagnostics",
                event = "history.create_failed",
                dictation_id = %completed.id,
                reason = %error
            );
            ui.set_history_status(ui.global::<Translations>().get_storage_error());
            ui.invoke_refresh_history();
        }
        DictationHistoryResult::Skipped(_) => {}
    }
    let play_finish_sound = completed.delivery.is_ok() && feedback_sounds_enabled;
    run_completion_feedback(play_finish_sound, play_feedback_sound, move || {
        dismiss_status_overlay(status_overlay, overlay_generation)
    });
    apply_transcription_completed(
        ui,
        completed.audio_duration_ms,
        &completed.processed,
        completed.delivery,
    );
    if requires_recovery && let Some(overlay) = result_overlay.upgrade() {
        show_result_overlay(completed.id, &overlay, &completed.processed.text);
    }
}

fn run_completion_feedback(
    play_finish_sound: bool,
    play_sound: impl FnOnce(FeedbackSound),
    dismiss_overlay: impl FnOnce() + Send + 'static,
) {
    if play_finish_sound {
        play_sound(FeedbackSound::Finish);
    }
    dismiss_overlay();
}

fn dismiss_status_overlay(status_overlay: slint::Weak<RecordingOverlay>, overlay_generation: i32) {
    if let Some(overlay) = status_overlay.upgrade()
        && overlay_generation_matches(overlay_generation, overlay.get_session_generation())
    {
        recording_runtime::animate_overlay_hide(&overlay, || {});
    }
}

fn show_result_overlay(
    id: template_app::DictationSessionId,
    overlay: &ResultOverlay,
    transcript: &str,
) {
    overlay.set_transcript(SharedString::from(transcript));
    if let Err(error) = overlay_window::present(overlay.window()) {
        tracing::warn!(
            target: "saymore::diagnostics",
            event = "delivery.recovery_present_failed",
            dictation_id = %id,
            reason = %error
        );
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex, MutexGuard};

    use super::*;

    #[test]
    fn finish_sound_starts_immediately_before_overlay_dismissal() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let play_events = Arc::clone(&events);
        let hide_events = Arc::clone(&events);

        run_completion_feedback(
            true,
            move |sound| {
                assert_eq!(FeedbackSound::Finish, sound);
                lock(&play_events).push("sound_started");
            },
            move || lock(&hide_events).push("overlay_dismissed"),
        );

        assert_eq!(
            &["sound_started", "overlay_dismissed"],
            lock(&events).as_slice()
        );
    }

    #[test]
    fn completion_without_finish_sound_dismisses_overlay_immediately() {
        let hidden = Arc::new(Mutex::new(false));
        let observed_hidden = Arc::clone(&hidden);

        run_completion_feedback(
            false,
            |_| unreachable!("sound must remain disabled"),
            move || *lock(&observed_hidden) = true,
        );

        assert!(*lock(&hidden));
    }

    fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
        match mutex.lock() {
            Ok(value) => value,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}
