use std::{
    io,
    sync::{Arc, Mutex},
};

use slint::ComponentHandle;
use template_app::{
    AccessibilityAuthorization, CorrectionObservingTextDeliverer, DeliveryTargetPrivacy,
    DictationCompletion, DictationCompletionAdapters, DictationCompletionClock,
    DictationCompletionPolicy, DictationCompletionResult, DictationHandoff,
    DictationHistoryMetadata, DictationHistoryPolicy, DictationPolicyError, DictationPolicySource,
    DictationSessionId, DictionaryLearningOutcome, DictionaryLearningStore,
    FinalTextProcessingError, FinalTextRequest, FinalTranscriptRefiner, LocalSettingsStore,
    NewDictionaryObservation, ProviderCatalog, ProviderInstance, RefinementEvaluation,
    TextDeliverer, TextDeliveryError, TextDeliveryOutcome, TextEditObserver, correction_from_edit,
};
use template_infra::{JsonSettingsStore, SqliteStorage, SystemClock, copy_text_to_clipboard};

use crate::{
    asr_runtime::AsrSessionController,
    refinement_runtime::{ProcessingActivity, RefinementPlan, RefinementRuntime},
    ui::{AppWindow, RecordingOverlay, Translations},
};

#[derive(Clone)]
pub(crate) struct DictationRuntime {
    pub(crate) asr: Arc<AsrSessionController>,
    refinement: Arc<RefinementRuntime>,
    storage: Arc<SqliteStorage>,
    settings: Arc<JsonSettingsStore>,
    deliverer: Arc<dyn CorrectionObservingTextDeliverer>,
}

pub(crate) struct CompletionContext {
    pub(crate) ui: slint::Weak<AppWindow>,
    pub(crate) status_overlay: slint::Weak<RecordingOverlay>,
    pub(crate) copy_to_clipboard: bool,
}

impl DictationRuntime {
    pub(crate) fn new(
        settings: Arc<JsonSettingsStore>,
        storage: Arc<SqliteStorage>,
        deliverer: Arc<dyn CorrectionObservingTextDeliverer>,
    ) -> Result<Self, io::Error> {
        let dictionary = storage.clone();
        Ok(Self {
            asr: Arc::new(AsrSessionController::new(settings.clone(), dictionary)),
            refinement: Arc::new(RefinementRuntime::new(settings.clone())?),
            storage,
            settings,
            deliverer,
        })
    }

    pub(crate) fn complete(
        &self,
        handoff: DictationHandoff,
        context: CompletionContext,
    ) -> DictationCompletionResult {
        let id = handoff.id();
        let policy = Arc::new(CompletionPolicyAdapter {
            refinement: Arc::clone(&self.refinement),
            storage: Arc::clone(&self.storage),
            settings: Arc::clone(&self.settings),
            plan: Mutex::new(None),
            ui: context.ui.clone(),
            status_overlay: context.status_overlay.clone(),
        });
        let observer = dictionary_edit_observer(
            id,
            Arc::clone(&self.storage),
            context.ui,
            Arc::clone(&self.refinement),
        );
        let deliverer = Arc::new(CompletionDeliverer::new(
            id,
            Arc::clone(&self.deliverer),
            observer,
            context.copy_to_clipboard,
        ));
        DictationCompletion::new(DictationCompletionAdapters {
            policy: policy.clone(),
            restored_transcriber: self.asr.clone(),
            refiner: policy,
            dictionary: self.storage.clone(),
            deliverer,
            history: self.storage.clone(),
            clock: Arc::new(SystemClock),
        })
        .complete(handoff)
    }
}

struct CompletionPolicyAdapter {
    refinement: Arc<RefinementRuntime>,
    storage: Arc<SqliteStorage>,
    settings: Arc<JsonSettingsStore>,
    plan: Mutex<Option<RefinementPlan>>,
    ui: slint::Weak<AppWindow>,
    status_overlay: slint::Weak<RecordingOverlay>,
}

impl DictationPolicySource for CompletionPolicyAdapter {
    fn load_policy(&self) -> Result<DictationCompletionPolicy, DictationPolicyError> {
        let local_settings = self
            .storage
            .load_settings()
            .map_err(|error| DictationPolicyError::Unavailable(error.to_string()))?;
        let (provider_settings, catalog) = self
            .settings
            .load_settings_snapshot()
            .map_err(|error| DictationPolicyError::Unavailable(error.to_string()))?;
        let plan = RefinementRuntime::plan_from_settings(&provider_settings);
        let refinement = plan.mode();
        let history = if local_settings.history_enabled {
            DictationHistoryPolicy::Enabled(history_metadata(&catalog))
        } else {
            DictationHistoryPolicy::Disabled
        };
        let mut stored_plan = self.plan.lock().map_err(|_| {
            DictationPolicyError::Unavailable("refinement plan lock was poisoned".to_owned())
        })?;
        *stored_plan = Some(plan);
        Ok(DictationCompletionPolicy {
            refinement,
            history,
        })
    }
}

impl FinalTranscriptRefiner for CompletionPolicyAdapter {
    fn refine(
        &self,
        id: DictationSessionId,
        request: FinalTextRequest,
    ) -> Result<RefinementEvaluation, FinalTextProcessingError> {
        let plan = self
            .plan
            .lock()
            .map_err(|_| FinalTextProcessingError::Cancelled)?
            .take()
            .ok_or(FinalTextProcessingError::Cancelled)?;
        let ui = self.ui.clone();
        let overlay = self.status_overlay.clone();
        self.refinement
            .refine_final_transcript(id, request, plan, move || {
                show_refining_activity(&ui, &overlay);
            })
    }
}

fn history_metadata(catalog: &ProviderCatalog) -> DictationHistoryMetadata {
    DictationHistoryMetadata {
        asr_provider_id: catalog.active.asr.clone(),
        llm_provider_id: catalog.active.llm.clone(),
        asr_model: active_provider_model(catalog.active.asr.as_deref(), &catalog.asr_providers),
        llm_model: active_provider_model(catalog.active.llm.as_deref(), &catalog.llm_providers),
    }
}

fn active_provider_model(
    active_id: Option<&str>,
    providers: &[ProviderInstance],
) -> Option<String> {
    providers
        .iter()
        .find(|provider| Some(provider.id.as_str()) == active_id)
        .and_then(|provider| provider.config.get("model"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
}

fn show_refining_activity(ui: &slint::Weak<AppWindow>, overlay: &slint::Weak<RecordingOverlay>) {
    let overlay = overlay.clone();
    let _ = ui.upgrade_in_event_loop(move |ui| {
        let label = ProcessingActivity::Refining.localized_label(&ui);
        ui.set_recording_status(label.clone());
        ui.set_recording_detail(label.clone());
        if let Some(overlay) = overlay.upgrade() {
            overlay.set_processing_label(label);
        }
    });
}

struct CompletionDeliverer {
    id: DictationSessionId,
    platform: Arc<dyn CorrectionObservingTextDeliverer>,
    observer: Mutex<Option<TextEditObserver>>,
    copy_to_clipboard: bool,
}

impl CompletionDeliverer {
    fn new(
        id: DictationSessionId,
        platform: Arc<dyn CorrectionObservingTextDeliverer>,
        observer: TextEditObserver,
        copy_to_clipboard: bool,
    ) -> Self {
        Self {
            id,
            platform,
            observer: Mutex::new(Some(observer)),
            copy_to_clipboard,
        }
    }
}

impl TextDeliverer for CompletionDeliverer {
    fn authorization(&self) -> AccessibilityAuthorization {
        self.platform.authorization()
    }

    fn request_authorization(&self) -> AccessibilityAuthorization {
        self.platform.request_authorization()
    }

    fn target_privacy(&self) -> DeliveryTargetPrivacy {
        self.platform.target_privacy()
    }

    fn deliver(&self, text: &str) -> Result<TextDeliveryOutcome, TextDeliveryError> {
        let observer = self
            .observer
            .lock()
            .map_err(|_| {
                TextDeliveryError::System("delivery observer lock was poisoned".to_owned())
            })?
            .take()
            .ok_or_else(|| {
                TextDeliveryError::System("dictation delivery was already attempted".to_owned())
            })?;
        let delivery = self.platform.deliver_and_observe(text, observer);
        if should_preserve_clipboard(self.copy_to_clipboard, &delivery)
            && let Err(error) = copy_text_to_clipboard(text)
        {
            tracing::warn!(
                target: "saymore::diagnostics",
                event = "delivery.clipboard_copy_failed",
                dictation_id = %self.id,
                reason = %error
            );
        }
        tracing::info!(
            target: "saymore::diagnostics",
            event = "delivery.completed",
            dictation_id = %self.id,
            result = ?delivery
        );
        delivery
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

fn dictionary_edit_observer(
    id: DictationSessionId,
    storage: Arc<SqliteStorage>,
    ui: slint::Weak<AppWindow>,
    refinement: Arc<RefinementRuntime>,
) -> TextEditObserver {
    let dictation_id = id.to_string();
    Box::new(move |edit| {
        let Some(correction) = correction_from_edit(&edit.original, &edit.edited) else {
            return;
        };
        let language = inferred_dictionary_language(&correction.canonical).to_owned();
        let assessment = refinement.assess_dictionary_correction(
            id,
            &correction.canonical,
            &edit.original,
            &edit.edited,
            &language,
        );
        let result = storage.record_dictionary_observation(NewDictionaryObservation {
            dictation_id: dictation_id.clone(),
            language,
            correction,
            assessment,
            observed_at_ms: SystemClock.now_ms(),
        });
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
            Ok(DictionaryLearningOutcome::Rejected | DictionaryLearningOutcome::Suppressed) => {}
            Err(error) => tracing::warn!(
                target: "saymore::diagnostics",
                event = "dictionary.learning_failed",
                dictation_id = %dictation_id,
                reason = %error
            ),
        });
    })
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
    use std::sync::atomic::{AtomicUsize, Ordering};

    use template_app::{ObservedTextEdit, TextEditObserver};

    use super::*;

    struct FakePlatformDeliverer {
        deliveries: Mutex<Vec<String>>,
        observer_calls: Arc<AtomicUsize>,
    }

    impl TextDeliverer for FakePlatformDeliverer {
        fn authorization(&self) -> AccessibilityAuthorization {
            AccessibilityAuthorization::Granted
        }

        fn request_authorization(&self) -> AccessibilityAuthorization {
            AccessibilityAuthorization::Granted
        }

        fn target_privacy(&self) -> DeliveryTargetPrivacy {
            DeliveryTargetPrivacy::Standard
        }

        fn deliver(&self, _text: &str) -> Result<TextDeliveryOutcome, TextDeliveryError> {
            Err(TextDeliveryError::System(
                "completion must use correction-observing delivery".to_owned(),
            ))
        }
    }

    impl CorrectionObservingTextDeliverer for FakePlatformDeliverer {
        fn deliver_and_observe(
            &self,
            text: &str,
            observer: TextEditObserver,
        ) -> Result<TextDeliveryOutcome, TextDeliveryError> {
            self.deliveries
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(text.to_owned());
            observer(ObservedTextEdit {
                original: text.to_owned(),
                edited: format!("{text}!"),
            });
            Ok(TextDeliveryOutcome::AccessibilityVerified)
        }
    }

    #[test]
    fn completion_delivery_wires_one_correction_observer() {
        let observer_calls = Arc::new(AtomicUsize::new(0));
        let platform = Arc::new(FakePlatformDeliverer {
            deliveries: Mutex::new(Vec::new()),
            observer_calls: Arc::clone(&observer_calls),
        });
        let observed = Arc::clone(&platform.observer_calls);
        let observer: TextEditObserver = Box::new(move |_| {
            observed.fetch_add(1, Ordering::Relaxed);
        });
        let deliverer = CompletionDeliverer::new(
            DictationSessionId::generate(),
            platform.clone(),
            observer,
            false,
        );

        assert_eq!(
            Ok(TextDeliveryOutcome::AccessibilityVerified),
            deliverer.deliver("hello")
        );
        assert_eq!(1, observer_calls.load(Ordering::Relaxed));
        assert_eq!(
            vec!["hello"],
            platform
                .deliveries
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        );
    }

    #[test]
    fn optional_clipboard_copy_excludes_sensitive_delivery() {
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
                "restricted".to_owned()
            ))
        ));
    }
}
