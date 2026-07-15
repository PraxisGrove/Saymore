use std::{
    io,
    sync::{Arc, Mutex},
    time::Instant,
};

use template_app::{
    ChatCompletionsLlmSettings, FinalTextProcessor, FinalTextRequest, ProcessedText,
    RefinementFallbackReason, RefinementMode, RefinementStatus, RefinementTerm, SettingsStore,
    SpeechRecognitionError, normalize_standard_spellings, relevant_dictionary_terms,
};
#[cfg(test)]
use template_infra::AppEnvironment;
use template_infra::{ChatCompletionsLlmProvider, JsonSettingsStore, SqliteStorage};
use tokio::runtime::Runtime;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefinementPlan {
    mode: RefinementMode,
    provider: Option<ChatCompletionsLlmSettings>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessingActivity {
    Transcribing,
    Refining,
}

impl ProcessingActivity {
    pub fn localized_label(self, ui: &crate::ui::AppWindow) -> slint::SharedString {
        use slint::ComponentHandle;

        let translations = ui.global::<crate::ui::Translations>();
        match self {
            Self::Transcribing => translations.get_recording_transcribing(),
            Self::Refining => translations.get_recording_refining(),
        }
    }
}

pub struct RefinementRuntime {
    settings: Arc<JsonSettingsStore>,
    processor: Mutex<Option<ProcessorCache>>,
    runtime: Runtime,
}

struct ProcessorCache {
    settings: ChatCompletionsLlmSettings,
    processor: Arc<FinalTextProcessor>,
}

pub struct ProcessedTranscript {
    pub processed: ProcessedText,
    pub llm_refined_text: Option<String>,
}

impl RefinementRuntime {
    pub fn new(settings: Arc<JsonSettingsStore>) -> Result<Self, io::Error> {
        Ok(Self {
            settings,
            processor: Mutex::new(None),
            runtime: Runtime::new()?,
        })
    }

    pub fn plan(&self) -> RefinementPlan {
        let Ok(settings) = self.settings.load() else {
            return disabled_plan();
        };
        let provider = settings.llm.chat_completions;
        let confirmed = !provider.base_url.trim().is_empty()
            && settings.llm.confirmed_base_url.trim() == provider.base_url.trim();
        if settings.llm.enabled && confirmed {
            RefinementPlan {
                mode: RefinementMode::Enabled,
                provider: Some(provider),
            }
        } else {
            disabled_plan()
        }
    }

    pub fn process_final_transcript<F>(
        &self,
        transcript: &str,
        plan: RefinementPlan,
        relevant_terms: Vec<RefinementTerm>,
        on_provider_attempt: F,
    ) -> Result<ProcessedTranscript, SpeechRecognitionError>
    where
        F: FnOnce(),
    {
        let transcript = crate::asr_runtime::normalize_transcript(transcript);
        if transcript.is_empty() {
            return Err(SpeechRecognitionError::Protocol(
                "empty transcript".to_owned(),
            ));
        }
        Ok(self.process(transcript, plan, relevant_terms, on_provider_attempt))
    }

    fn process<F>(
        &self,
        transcript: String,
        plan: RefinementPlan,
        relevant_terms: Vec<RefinementTerm>,
        on_provider_attempt: F,
    ) -> ProcessedTranscript
    where
        F: FnOnce(),
    {
        let started = Instant::now();
        let fallback_text = transcript.clone();
        let processor = self.processor_for(plan.provider);
        let mut request = FinalTextRequest::new(transcript, plan.mode);
        request.relevant_terms = relevant_terms.clone();
        request.language = Some(inferred_transcript_language(&request.transcript).to_owned());
        let mut processed = match self
            .runtime
            .block_on(processor.process_with_attempt_observer(
                request,
                CancellationToken::new(),
                on_provider_attempt,
            )) {
            Ok(processed) => processed,
            Err(_) => ProcessedText {
                text: fallback_text,
                refinement: RefinementStatus::FellBack(RefinementFallbackReason::Protocol),
            },
        };
        let llm_refined_text = matches!(processed.refinement, RefinementStatus::Completed)
            .then(|| processed.text.clone());
        processed.text = normalize_standard_spellings(&processed.text, &relevant_terms);
        log_refinement_result(&processed.refinement, started.elapsed().as_millis());
        ProcessedTranscript {
            processed,
            llm_refined_text,
        }
    }

    fn processor_for(
        &self,
        settings: Option<ChatCompletionsLlmSettings>,
    ) -> Arc<FinalTextProcessor> {
        let Some(settings) = settings else {
            return Arc::new(FinalTextProcessor::unconfigured());
        };
        let Ok(mut cache) = self.processor.lock() else {
            return Arc::new(FinalTextProcessor::unconfigured());
        };
        if let Some(cached) = cache.as_ref()
            && cached.settings == settings
        {
            return Arc::clone(&cached.processor);
        }
        let processor = ChatCompletionsLlmProvider::new(settings.clone())
            .map(|provider| Arc::new(FinalTextProcessor::configured(Arc::new(provider))))
            .unwrap_or_else(|_| Arc::new(FinalTextProcessor::unconfigured()));
        *cache = Some(ProcessorCache {
            settings,
            processor: Arc::clone(&processor),
        });
        processor
    }
}

pub fn relevant_terms_for_transcript(
    storage: &SqliteStorage,
    transcript: &str,
) -> Vec<RefinementTerm> {
    relevant_dictionary_terms(
        storage,
        transcript,
        inferred_transcript_language(transcript),
    )
    .unwrap_or_default()
}

fn inferred_transcript_language(text: &str) -> &'static str {
    if text.chars().any(
        |character| matches!(character as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF),
    ) {
        "zh-Hans"
    } else {
        "en"
    }
}

fn log_refinement_result(status: &RefinementStatus, duration_ms: u128) {
    match status {
        RefinementStatus::Disabled => {
            tracing::info!(target: "saymore::diagnostics", event = "llm.disabled", duration_ms);
        }
        RefinementStatus::Skipped(_) => {
            tracing::info!(target: "saymore::diagnostics",
                event = "llm.skipped",
                reason = "short_transcript",
                duration_ms
            );
        }
        RefinementStatus::Completed => {
            tracing::info!(target: "saymore::diagnostics", event = "llm.completed", duration_ms);
        }
        RefinementStatus::FellBack(reason) => {
            tracing::warn!(target: "saymore::diagnostics", event = "llm.fallback", reason = ?reason, duration_ms);
        }
    }
}

fn disabled_plan() -> RefinementPlan {
    RefinementPlan {
        mode: RefinementMode::Disabled,
        provider: None,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        sync::atomic::{AtomicBool, Ordering},
    };

    use httpmock::{Method::POST, MockServer};

    use super::*;

    #[test]
    fn infers_the_minimal_refinement_language_hint() {
        assert_eq!("zh-Hans", inferred_transcript_language("你好 OpenAI"));
        assert_eq!("en", inferred_transcript_language("hello OpenAI"));
    }

    #[test]
    fn disabled_plan_returns_the_transcript_without_a_provider_call() {
        let runtime = test_runtime();

        let Ok(processed) =
            runtime.process_final_transcript("  原始文本。  ", disabled_plan(), Vec::new(), || {})
        else {
            panic!("non-empty transcript should be processed");
        };

        assert_eq!("原始文本。", processed.processed.text);
        assert_eq!(RefinementStatus::Disabled, processed.processed.refinement);
        assert_eq!(None, processed.llm_refined_text);
    }

    #[test]
    fn standard_spelling_runs_after_the_optional_refinement_stage() {
        let runtime = test_runtime();
        let terms = vec![RefinementTerm {
            canonical: "OpenAI".to_owned(),
        }];

        let Ok(processed) = runtime.process_final_transcript(
            "openai、ＯｐｅｎＡＩ，但不要合并 open ai。",
            disabled_plan(),
            terms,
            || {},
        ) else {
            panic!("non-empty transcript should be processed");
        };

        assert_eq!(
            "OpenAI、OpenAI，但不要合并 open ai。",
            processed.processed.text
        );
        assert_eq!(None, processed.llm_refined_text);
    }

    #[test]
    fn configured_plan_completes_through_the_provider() {
        let server = MockServer::start();
        let completion = server.mock(|when, then| {
            when.method(POST).path("/v1/chat/completions");
            then.status(200)
                .header("content-type", "application/json")
                .body(r#"{"choices":[{"message":{"content":"原始文本。"}}]}"#);
        });
        let runtime = test_runtime();
        let plan = RefinementPlan {
            mode: RefinementMode::Enabled,
            provider: Some(provider_settings(server.url("/v1"))),
        };

        let transcript = "今天先完成登录测试，明天处理设置页面，发布前检查配置迁移。";
        let attempted = AtomicBool::new(false);
        let Ok(processed) = runtime.process_final_transcript(transcript, plan, Vec::new(), || {
            attempted.store(true, Ordering::Relaxed)
        }) else {
            panic!("non-empty transcript should be processed");
        };

        assert_eq!(RefinementStatus::Completed, processed.processed.refinement);
        assert_eq!(Some("原始文本。".to_owned()), processed.llm_refined_text);
        assert!(attempted.load(Ordering::Relaxed));
        completion.assert();
    }

    #[test]
    fn short_transcript_does_not_report_a_provider_attempt() {
        let runtime = test_runtime();
        let plan = RefinementPlan {
            mode: RefinementMode::Enabled,
            provider: Some(ChatCompletionsLlmSettings::default()),
        };
        let attempted = AtomicBool::new(false);

        let Ok(processed) =
            runtime.process_final_transcript("好的，谢谢。", plan, Vec::new(), || {
                attempted.store(true, Ordering::Relaxed)
            })
        else {
            panic!("non-empty transcript should be processed");
        };

        assert!(matches!(
            processed.processed.refinement,
            RefinementStatus::Skipped(_)
        ));
        assert_eq!(None, processed.llm_refined_text);
        assert!(!attempted.load(Ordering::Relaxed));
    }

    #[test]
    fn provider_failure_falls_back_to_the_transcript() {
        let server = MockServer::start();
        let rejected = server.mock(|when, then| {
            when.method(POST).path("/v1/chat/completions");
            then.status(401);
        });
        let runtime = test_runtime();
        let plan = RefinementPlan {
            mode: RefinementMode::Enabled,
            provider: Some(provider_settings(server.url("/v1"))),
        };

        let transcript = "今天先完成登录测试，明天处理设置页面，发布前检查配置迁移。";
        let Ok(processed) = runtime.process_final_transcript(transcript, plan, Vec::new(), || {})
        else {
            panic!("non-empty transcript should be processed");
        };

        assert_eq!(transcript, processed.processed.text);
        assert_eq!(
            RefinementStatus::FellBack(RefinementFallbackReason::Authentication),
            processed.processed.refinement
        );
        assert_eq!(None, processed.llm_refined_text);
        rejected.assert();
    }

    #[test]
    fn rejects_an_empty_normalized_transcript() {
        let runtime = test_runtime();

        let result = runtime.process_final_transcript(" \n ", disabled_plan(), Vec::new(), || {});

        assert!(matches!(result, Err(SpeechRecognitionError::Protocol(_))));
    }

    #[test]
    #[ignore = "uses the current user's live LLM configuration"]
    fn current_user_configuration_runs_the_desktop_pipeline()
    -> Result<(), Box<dyn std::error::Error>> {
        let settings = Arc::new(JsonSettingsStore::for_current_user(
            AppEnvironment::Production,
        )?);
        let runtime = RefinementRuntime::new(settings)?;
        let plan = runtime.plan();
        let processed = runtime.process_final_transcript(
            "这个真的真的很重要，而且我们今天需要先完成测试，再决定下一步怎么处理。",
            plan,
            Vec::new(),
            || {},
        )?;
        if processed.processed.refinement != RefinementStatus::Completed {
            return Err("desktop refinement pipeline did not complete".into());
        }
        Ok(())
    }

    fn test_runtime() -> RefinementRuntime {
        let Ok(settings) = JsonSettingsStore::for_current_user(AppEnvironment::Production) else {
            panic!("current user settings path should be available");
        };
        let Ok(runtime) = RefinementRuntime::new(Arc::new(settings)) else {
            panic!("Tokio runtime should be available");
        };
        runtime
    }

    fn provider_settings(base_url: String) -> ChatCompletionsLlmSettings {
        ChatCompletionsLlmSettings {
            base_url,
            api_key: "test-key".to_owned(),
            model: "test-model".to_owned(),
            custom_headers: BTreeMap::new(),
        }
    }
}
