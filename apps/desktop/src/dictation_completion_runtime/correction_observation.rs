use std::{
    sync::{Arc, Mutex},
    thread,
};

use slint::ComponentHandle;
use template_app::{
    DictationCompletionClock, DictationCompletionResult, DictationHistoryResult,
    DictationSessionId, DictionaryLearningOutcome, DictionaryLearningStore, FinalTextRevision,
    FinalTextRevisionState, HistoryStore, NewDictionaryObservation, TextRevisionEvent,
    TextRevisionObserver,
};
use template_infra::{SqliteStorage, SystemClock};

use crate::{
    refinement_runtime::RefinementRuntime,
    ui::{AppWindow, Translations},
};

pub(super) struct HistoryRevisionRecorder {
    dictation_id: String,
    storage: Arc<SqliteStorage>,
    ui: slint::Weak<AppWindow>,
    state: Mutex<HistoryRevisionState>,
}

impl HistoryRevisionRecorder {
    pub(super) fn new(
        id: DictationSessionId,
        storage: Arc<SqliteStorage>,
        ui: slint::Weak<AppWindow>,
    ) -> Self {
        Self {
            dictation_id: id.to_string(),
            storage,
            ui,
            state: Mutex::new(HistoryRevisionState::Awaiting(None)),
        }
    }

    pub(super) fn observe(&self, final_text: String) {
        let update = match self.state.lock() {
            Ok(mut state) => state.observe(final_text),
            Err(_) => {
                self.log_failure("history revision state lock was poisoned");
                return;
            }
        };
        if let Some(final_text) = update {
            self.persist(&final_text);
        }
    }

    pub(super) fn finish(&self, history_saved: bool) {
        let update = match self.state.lock() {
            Ok(mut state) => state.finish(history_saved),
            Err(_) => {
                self.log_failure("history revision state lock was poisoned");
                return;
            }
        };
        if let Some(final_text) = update {
            self.persist(&final_text);
        }
    }

    fn persist(&self, final_text: &str) {
        match self
            .storage
            .update_history_final_text(&self.dictation_id, final_text)
        {
            Ok(()) => {
                let _ = self
                    .ui
                    .upgrade_in_event_loop(|ui| ui.invoke_refresh_history());
            }
            Err(error) => self.log_failure(&error.to_string()),
        }
    }

    fn log_failure(&self, reason: &str) {
        tracing::warn!(
            target: "saymore::diagnostics",
            event = "history.revision_update_failed",
            dictation_id = %self.dictation_id,
            reason
        );
    }
}

enum HistoryRevisionState {
    Awaiting(Option<String>),
    Active,
    Inactive,
}

impl HistoryRevisionState {
    fn observe(&mut self, final_text: String) -> Option<String> {
        match self {
            Self::Awaiting(pending) => {
                *pending = Some(final_text);
                None
            }
            Self::Active => Some(final_text),
            Self::Inactive => None,
        }
    }

    fn finish(&mut self, history_saved: bool) -> Option<String> {
        let next = if history_saved {
            Self::Active
        } else {
            Self::Inactive
        };
        match std::mem::replace(self, next) {
            Self::Awaiting(pending) if history_saved => pending,
            Self::Awaiting(_) | Self::Active | Self::Inactive => None,
        }
    }
}

pub(super) fn history_was_saved(result: &DictationCompletionResult) -> bool {
    matches!(
        result,
        DictationCompletionResult::Completed(completed)
            if matches!(completed.history, DictationHistoryResult::Saved(_))
    )
}

pub(super) fn text_revision_observer(
    original: String,
    id: DictationSessionId,
    storage: Arc<SqliteStorage>,
    ui: slint::Weak<AppWindow>,
    refinement: Arc<RefinementRuntime>,
    history_revision: Arc<HistoryRevisionRecorder>,
) -> TextRevisionObserver {
    let dictation_id = id.to_string();
    let state = Mutex::new(FinalTextRevisionState::new(original));
    Box::new(move |event: TextRevisionEvent| {
        let revision = match state.lock() {
            Ok(mut state) => state.handle(event),
            Err(_) => {
                tracing::warn!(
                    target: "saymore::diagnostics",
                    event = "history.revision_state_unavailable"
                );
                return;
            }
        };
        let Some(revision) = revision else {
            return;
        };
        history_revision.observe(revision.final_text.clone());
        if !revision.permits_dictionary_learning() {
            return;
        }
        let revision_storage = Arc::clone(&storage);
        let revision_ui = ui.clone();
        let revision_refinement = Arc::clone(&refinement);
        let revision_dictation_id = dictation_id.clone();
        if let Err(error) = thread::Builder::new()
            .name("saymore-dictionary-correction".to_owned())
            .spawn(move || {
                record_dictionary_revision(
                    id,
                    revision_dictation_id,
                    revision_storage,
                    revision_ui,
                    revision_refinement,
                    revision,
                );
            })
        {
            tracing::warn!(
                target: "saymore::diagnostics",
                event = "dictionary.correction_worker_failed",
                reason = %error
            );
        }
    })
}

#[allow(clippy::too_many_arguments)]
fn record_dictionary_revision(
    id: DictationSessionId,
    dictation_id: String,
    storage: Arc<SqliteStorage>,
    ui: slint::Weak<AppWindow>,
    refinement: Arc<RefinementRuntime>,
    revision: FinalTextRevision,
) {
    let language = inferred_dictionary_language(&revision.final_text).to_owned();
    let reviews = refinement.assess_dictionary_revision(
        id,
        &revision.original,
        &revision.final_text,
        &language,
    );
    for review in reviews {
        let result = storage.record_dictionary_observation(NewDictionaryObservation {
            dictation_id: dictation_id.clone(),
            language: language.clone(),
            correction: review.correction,
            assessment: review.assessment,
            observed_at_ms: SystemClock.now_ms(),
        });
        let event_dictation_id = dictation_id.clone();
        let _ = ui.upgrade_in_event_loop(move |ui| match result {
            Ok(DictionaryLearningOutcome::Added(entry)) => {
                ui.set_dictionary_status(
                    ui.global::<Translations>()
                        .invoke_dictionary_automatically_added(entry.canonical.clone().into()),
                );
                ui.invoke_refresh_dictionary();
                ui.invoke_show_dictionary_added(entry.id.into(), entry.canonical.into());
            }
            Ok(DictionaryLearningOutcome::Pending { .. }) => ui.invoke_refresh_dictionary(),
            Ok(
                DictionaryLearningOutcome::AlreadyPresent
                | DictionaryLearningOutcome::Rejected
                | DictionaryLearningOutcome::Suppressed,
            ) => {}
            Err(error) => tracing::warn!(
                target: "saymore::diagnostics",
                event = "dictionary.learning_failed",
                dictation_id = %event_dictation_id,
                reason = %error
            ),
        });
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_revision_keeps_the_latest_pending_and_active_revision() {
        let mut saved = HistoryRevisionState::Awaiting(None);
        assert_eq!(None, saved.observe("corrected early".to_owned()));
        assert_eq!(None, saved.observe("corrected latest".to_owned()));
        assert_eq!(Some("corrected latest".to_owned()), saved.finish(true));
        assert_eq!(
            Some("corrected later".to_owned()),
            saved.observe("corrected later".to_owned())
        );

        let mut skipped = HistoryRevisionState::Awaiting(None);
        assert_eq!(None, skipped.observe("must not persist".to_owned()));
        assert_eq!(None, skipped.finish(false));
        assert_eq!(None, skipped.observe("still ignored".to_owned()));
    }
}
