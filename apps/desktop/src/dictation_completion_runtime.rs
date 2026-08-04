use std::{
    io,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use template_app::{
    AccessibilityAuthorization, CorrectionObservingTextDeliverer, DeliveryTargetPrivacy,
    DictationCompletion, DictationCompletionAdapters, DictationCompletionPolicy,
    DictationCompletionResult, DictationHandoff, DictationHistoryMetadata, DictationHistoryPolicy,
    DictationPolicyError, DictationPolicySource, DictationSessionId, FinalTextProcessingError,
    FinalTextRequest, FinalTranscriptRefiner, LocalSettingsStore, ProviderCatalog,
    ProviderInstance, RefinementEvaluation, TextDeliverer, TextDeliveryError, TextDeliveryOutcome,
    TextRevisionEndReason, TextRevisionObserver,
};
use template_infra::{
    JsonSettingsStore, PARAFORMER_MODEL_ID, QWEN3_ASR_MODEL_ID, SENSE_VOICE_MODEL_ID,
    SqliteStorage, SystemClock, WHISPER_MODEL_ID, copy_text_to_clipboard,
};

use crate::{
    asr_runtime::AsrSessionController,
    refinement_runtime::{ProcessingActivity, RefinementPlan, RefinementRuntime},
    ui::{AppWindow, RecordingOverlay},
};

mod correction_observation;

use correction_observation::{HistoryRevisionRecorder, history_was_saved, text_revision_observer};

const MACOS_SPEECH_PROVIDER_ID: &str = "macos-speech";
const MACOS_DICTATION_MODEL_ID: &str = "macos-dictation";

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
        models_directory: PathBuf,
    ) -> Result<Self, io::Error> {
        let dictionary = storage.clone();
        Ok(Self {
            asr: Arc::new(AsrSessionController::new(
                settings.clone(),
                dictionary,
                models_directory,
            )),
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
        let history_revision = Arc::new(HistoryRevisionRecorder::new(
            id,
            Arc::clone(&self.storage),
            context.ui.clone(),
        ));
        let observer_factory: TextRevisionObserverFactory = Box::new({
            let storage = Arc::clone(&self.storage);
            let refinement = Arc::clone(&self.refinement);
            let history_revision = Arc::clone(&history_revision);
            move |original| {
                text_revision_observer(
                    original,
                    id,
                    storage,
                    context.ui,
                    refinement,
                    history_revision,
                )
            }
        });
        let deliverer = Arc::new(CompletionDeliverer::new(
            id,
            Arc::clone(&self.deliverer),
            observer_factory,
            context.copy_to_clipboard,
        ));
        let result = DictationCompletion::new(DictationCompletionAdapters {
            policy: policy.clone(),
            restored_transcriber: self.asr.clone(),
            refiner: policy,
            dictionary: self.storage.clone(),
            deliverer,
            history: self.storage.clone(),
            clock: Arc::new(SystemClock),
        })
        .complete(handoff);
        history_revision.finish(history_was_saved(&result));
        result
    }

    pub(crate) fn finish_text_revision_observation(&self, reason: TextRevisionEndReason) {
        self.deliverer.finish_observation(reason);
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
        asr_model: active_asr_model(catalog.active.asr.as_deref(), &catalog.asr_providers),
        llm_model: active_provider_model(catalog.active.llm.as_deref(), &catalog.llm_providers),
    }
}

fn active_asr_model(active_id: Option<&str>, providers: &[ProviderInstance]) -> Option<String> {
    active_provider_model(active_id, providers).or_else(|| {
        let model = match active_id? {
            MACOS_SPEECH_PROVIDER_ID => MACOS_DICTATION_MODEL_ID,
            template_app::PARAFORMER_PROVIDER_ID => PARAFORMER_MODEL_ID,
            template_app::WHISPER_PROVIDER_ID => WHISPER_MODEL_ID,
            template_app::QWEN3_ASR_PROVIDER_ID => QWEN3_ASR_MODEL_ID,
            template_app::SENSE_VOICE_PROVIDER_ID => SENSE_VOICE_MODEL_ID,
            _ => return None,
        };
        Some(model.to_owned())
    })
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
    observer_factory: Mutex<Option<TextRevisionObserverFactory>>,
    copy_to_clipboard: bool,
}

type TextRevisionObserverFactory = Box<dyn FnOnce(String) -> TextRevisionObserver + Send + 'static>;

impl CompletionDeliverer {
    fn new(
        id: DictationSessionId,
        platform: Arc<dyn CorrectionObservingTextDeliverer>,
        observer_factory: TextRevisionObserverFactory,
        copy_to_clipboard: bool,
    ) -> Self {
        Self {
            id,
            platform,
            observer_factory: Mutex::new(Some(observer_factory)),
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
        let observer_factory = self
            .observer_factory
            .lock()
            .map_err(|_| {
                TextDeliveryError::System("delivery observer lock was poisoned".to_owned())
            })?
            .take()
            .ok_or_else(|| {
                TextDeliveryError::System("dictation delivery was already attempted".to_owned())
            })?;
        let observer = observer_factory(text.to_owned());
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use template_app::{TextRevisionEndReason, TextRevisionEvent, TextRevisionObserver};

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
            observer: TextRevisionObserver,
        ) -> Result<TextDeliveryOutcome, TextDeliveryError> {
            self.deliveries
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(text.to_owned());
            observer(TextRevisionEvent::Snapshot(format!("{text}!")));
            observer(TextRevisionEvent::Snapshot(format!("{text}!!")));
            observer(TextRevisionEvent::Ended(TextRevisionEndReason::FocusLost));
            Ok(TextDeliveryOutcome::AccessibilityVerified)
        }

        fn finish_observation(&self, _reason: TextRevisionEndReason) {}
    }

    #[test]
    fn completion_delivery_keeps_the_correction_observer_active() {
        let observer_calls = Arc::new(AtomicUsize::new(0));
        let platform = Arc::new(FakePlatformDeliverer {
            deliveries: Mutex::new(Vec::new()),
            observer_calls: Arc::clone(&observer_calls),
        });
        let observed = Arc::clone(&platform.observer_calls);
        let observer_factory: TextRevisionObserverFactory = Box::new(move |_| {
            Box::new(move |_| {
                observed.fetch_add(1, Ordering::Relaxed);
            })
        });
        let deliverer = CompletionDeliverer::new(
            DictationSessionId::generate(),
            platform.clone(),
            observer_factory,
            false,
        );

        assert_eq!(
            Ok(TextDeliveryOutcome::AccessibilityVerified),
            deliverer.deliver("hello")
        );
        assert_eq!(3, observer_calls.load(Ordering::Relaxed));
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

    #[test]
    fn history_metadata_records_model_ids_for_built_in_asr_providers() {
        let mut macos = ProviderCatalog::default();
        macos.select_macos_speech_provider();
        let mut paraformer = ProviderCatalog::default();
        paraformer.select_paraformer_provider();
        let mut whisper = ProviderCatalog::default();
        whisper.select_whisper_provider();
        let mut qwen3 = ProviderCatalog::default();
        qwen3.select_qwen3_asr_provider();
        let mut sense_voice = ProviderCatalog::default();
        sense_voice.select_sense_voice_provider();
        let cases = [
            (macos, MACOS_DICTATION_MODEL_ID),
            (paraformer, PARAFORMER_MODEL_ID),
            (whisper, WHISPER_MODEL_ID),
            (qwen3, QWEN3_ASR_MODEL_ID),
            (sense_voice, SENSE_VOICE_MODEL_ID),
        ];

        for (catalog, expected_model) in cases {
            assert_eq!(
                Some(expected_model),
                history_metadata(&catalog).asr_model.as_deref()
            );
        }
    }
}
