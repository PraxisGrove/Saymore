use slint::ComponentHandle;
use template_app::{HistoryRefinementRetryError, RefinementFallbackReason, RefinementStatus};

use crate::{
    dictation_completion_runtime::DictationRuntime,
    ui::{AppWindow, HistoryRepolishState},
};

/// Lets the history detail dialog polish a saved dictation again.
pub(crate) fn wire(ui: &AppWindow, dictation: DictationRuntime) {
    let weak_ui = ui.as_weak();
    ui.on_repolish_history(move |id, transcript| {
        let Some(ui) = weak_ui.upgrade() else {
            return;
        };
        if ui.get_history_repolish_state() == HistoryRepolishState::Running {
            return;
        }
        ui.set_history_repolish_state(HistoryRepolishState::Running);
        let dictation = dictation.clone();
        let completion_ui = weak_ui.clone();
        let id = id.to_string();
        let transcript = transcript.to_string();
        let spawned = std::thread::Builder::new()
            .name("saymore-history-repolish".to_owned())
            .spawn(move || {
                let result = dictation.retry_history_refinement(id.clone(), transcript);
                if let Ok(text) = &result
                    && let Err(error) = template_infra::copy_text_to_clipboard(text)
                {
                    tracing::warn!(event = "history.repolish_copy_failed", reason = %error);
                }
                let _ = completion_ui
                    .upgrade_in_event_loop(move |ui| apply_repolish_result(&ui, &id, result));
            });
        if spawned.is_err() {
            ui.set_history_repolish_state(HistoryRepolishState::Failed);
        }
    });
}

fn apply_repolish_result(
    ui: &AppWindow,
    id: &str,
    result: Result<String, HistoryRefinementRetryError>,
) {
    let mut item = ui.get_history_selected_item();
    let still_selected = item.id.as_str() == id;
    ui.set_history_repolish_state(if still_selected {
        repolish_state(&result)
    } else {
        HistoryRepolishState::Idle
    });
    if let Ok(text) = result {
        if still_selected {
            item.text = text.as_str().into();
            item.llm_text = text.into();
            ui.set_history_selected_item(item);
        }
        ui.invoke_refresh_history();
    }
}

fn repolish_state(result: &Result<String, HistoryRefinementRetryError>) -> HistoryRepolishState {
    let Err(error) = result else {
        return HistoryRepolishState::Succeeded;
    };
    let HistoryRefinementRetryError::NotRefined(RefinementStatus::FellBack(reason)) = error else {
        return HistoryRepolishState::Failed;
    };
    match reason {
        RefinementFallbackReason::NotConfigured
        | RefinementFallbackReason::Authentication
        | RefinementFallbackReason::InvalidConfiguration
        | RefinementFallbackReason::ModelUnavailable => HistoryRepolishState::NotConfigured,
        RefinementFallbackReason::Timeout
        | RefinementFallbackReason::Transport
        | RefinementFallbackReason::Quota
        | RefinementFallbackReason::Protocol
        | RefinementFallbackReason::TemporarilyUnavailable => HistoryRepolishState::Unavailable,
        RefinementFallbackReason::OutputRejected(_) => HistoryRepolishState::Rejected,
    }
}
