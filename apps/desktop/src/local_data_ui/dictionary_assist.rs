use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use slint::{ComponentHandle, Timer};
use template_app::{
    ProviderConfigStore, VocabularySuggestionRunOutcome, run_vocabulary_suggestions_if_due,
    vocabulary_suggestion_consent_fingerprint,
};
use template_infra::{ChatCompletionsLlmProvider, JsonSettingsStore, SqliteStorage};

use super::{now_ms, spawn_named};
use crate::ui::AppWindow;

const CHECK_INTERVAL: Duration = Duration::from_secs(60 * 60);

#[derive(Clone)]
pub(super) struct VocabularySuggestionTrigger {
    ui: slint::Weak<AppWindow>,
    storage: Arc<SqliteStorage>,
    provider_settings: Arc<JsonSettingsStore>,
    running: Arc<AtomicBool>,
}

pub(super) fn wire(
    ui: &AppWindow,
    storage: Arc<SqliteStorage>,
    provider_settings: Arc<JsonSettingsStore>,
) -> VocabularySuggestionTrigger {
    let trigger = VocabularySuggestionTrigger {
        ui: ui.as_weak(),
        storage,
        provider_settings,
        running: Arc::new(AtomicBool::new(false)),
    };
    let initial = trigger.clone();
    Timer::single_shot(Duration::from_secs(1), move || initial.run_if_due());
    schedule_next_check(trigger.clone());
    trigger
}

impl VocabularySuggestionTrigger {
    pub(super) fn run_if_due(&self) {
        if self
            .running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let task = self.clone();
        spawn_named("saymore-dictionary-assist", move || {
            let result = task.analyze_if_due();
            task.running.store(false, Ordering::Release);
            match result {
                Ok(Some(_completed_at_ms)) => {}
                Ok(None) => {}
                Err(error) => tracing::warn!(
                    target: "saymore::diagnostics",
                    event = "dictionary.assist_failed",
                    reason = %error
                ),
            }
        });
    }

    fn analyze_if_due(&self) -> Result<Option<i64>, String> {
        let now = now_ms();
        let catalog = self
            .provider_settings
            .load_catalog()
            .map_err(|error| error.to_string())?;
        let Some(provider_preset) = catalog.active_llm_provider() else {
            return Ok(None);
        };
        if catalog
            .configured_llm_provider_model(provider_preset)
            .is_none()
        {
            return Ok(None);
        }
        let Some(provider_settings) = catalog.llm_provider_settings(provider_preset) else {
            return Ok(None);
        };
        let provider_id = catalog
            .active
            .llm
            .as_deref()
            .unwrap_or_else(|| provider_preset.id());
        let consent_fingerprint =
            vocabulary_suggestion_consent_fingerprint(provider_id, &provider_settings.base_url);
        let provider = ChatCompletionsLlmProvider::new(provider_settings)
            .map_err(|error| error.to_string())?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| error.to_string())?;
        match runtime
            .block_on(run_vocabulary_suggestions_if_due(
                &provider,
                self.storage.as_ref(),
                self.storage.as_ref(),
                self.storage.as_ref(),
                &consent_fingerprint,
                now,
            ))
            .map_err(|error| error.to_string())?
        {
            VocabularySuggestionRunOutcome::Skipped
            | VocabularySuggestionRunOutcome::Interrupted => Ok(None),
            VocabularySuggestionRunOutcome::Completed {
                completed_at_ms,
                dictionary_changed,
            } => {
                if dictionary_changed {
                    let _ = self
                        .ui
                        .upgrade_in_event_loop(|ui| ui.invoke_refresh_dictionary());
                }
                Ok(Some(completed_at_ms))
            }
        }
    }
}

fn schedule_next_check(trigger: VocabularySuggestionTrigger) {
    Timer::single_shot(CHECK_INTERVAL, move || {
        trigger.run_if_due();
        schedule_next_check(trigger);
    });
}
