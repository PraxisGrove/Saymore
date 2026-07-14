use std::{
    io,
    sync::{Arc, Mutex},
};

use template_app::{
    ChatCompletionsLlmSettings, FinalTextProcessor, FinalTextRequest, ProcessedText,
    RefinementFallbackReason, RefinementMode, RefinementStatus, SettingsStore,
    SpeechRecognitionError,
};
use template_infra::{ChatCompletionsLlmProvider, JsonSettingsStore};
use tokio::runtime::Runtime;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefinementPlan {
    mode: RefinementMode,
    provider: Option<ChatCompletionsLlmSettings>,
}

impl RefinementPlan {
    pub fn processing_label(&self) -> &'static str {
        match self.mode {
            RefinementMode::Disabled => "正在转写",
            RefinementMode::Enabled => "正在润色",
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

    pub fn process_final_transcript(
        &self,
        transcript: &str,
        plan: RefinementPlan,
    ) -> Result<ProcessedText, SpeechRecognitionError> {
        let transcript = crate::asr_runtime::normalize_transcript(transcript);
        if transcript.is_empty() {
            return Err(SpeechRecognitionError::Protocol(
                "empty transcript".to_owned(),
            ));
        }
        Ok(self.process(transcript, plan))
    }

    fn process(&self, transcript: String, plan: RefinementPlan) -> ProcessedText {
        let fallback_text = transcript.clone();
        let processor = self.processor_for(plan.provider);
        let request = FinalTextRequest::new(transcript, plan.mode);
        match self
            .runtime
            .block_on(processor.process(request, CancellationToken::new()))
        {
            Ok(processed) => processed,
            Err(_) => ProcessedText {
                text: fallback_text,
                refinement: RefinementStatus::FellBack(RefinementFallbackReason::Protocol),
            },
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

fn disabled_plan() -> RefinementPlan {
    RefinementPlan {
        mode: RefinementMode::Disabled,
        provider: None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use httpmock::{Method::POST, MockServer};

    use super::*;

    #[test]
    fn processing_copy_depends_only_on_captured_mode() {
        assert_eq!("正在转写", disabled_plan().processing_label());
        assert_eq!(
            "正在润色",
            RefinementPlan {
                mode: RefinementMode::Enabled,
                provider: Some(ChatCompletionsLlmSettings::default()),
            }
            .processing_label()
        );
    }

    #[test]
    fn disabled_plan_returns_the_transcript_without_a_provider_call() {
        let runtime = test_runtime();

        let Ok(processed) = runtime.process_final_transcript("  原始文本。  ", disabled_plan())
        else {
            panic!("non-empty transcript should be processed");
        };

        assert_eq!("原始文本。", processed.text);
        assert_eq!(RefinementStatus::Disabled, processed.refinement);
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

        let Ok(processed) = runtime.process_final_transcript("原始文本。", plan) else {
            panic!("non-empty transcript should be processed");
        };

        assert_eq!(RefinementStatus::Completed, processed.refinement);
        completion.assert();
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

        let Ok(processed) = runtime.process_final_transcript("原始文本。", plan) else {
            panic!("non-empty transcript should be processed");
        };

        assert_eq!("原始文本。", processed.text);
        assert_eq!(
            RefinementStatus::FellBack(RefinementFallbackReason::Authentication),
            processed.refinement
        );
        rejected.assert();
    }

    #[test]
    fn rejects_an_empty_normalized_transcript() {
        let runtime = test_runtime();

        let result = runtime.process_final_transcript(" \n ", disabled_plan());

        assert!(matches!(result, Err(SpeechRecognitionError::Protocol(_))));
    }

    #[test]
    #[ignore = "uses the current user's live LLM configuration"]
    fn current_user_configuration_runs_the_desktop_pipeline()
    -> Result<(), Box<dyn std::error::Error>> {
        let settings = Arc::new(JsonSettingsStore::for_current_user()?);
        let runtime = RefinementRuntime::new(settings)?;
        let plan = runtime.plan();
        let processed = runtime.process_final_transcript("这个真的真的很重要。", plan)?;
        if processed.refinement != RefinementStatus::Completed
            || processed.text != "这个真的真的很重要。"
        {
            return Err("desktop refinement pipeline did not complete".into());
        }
        Ok(())
    }

    fn test_runtime() -> RefinementRuntime {
        let Ok(settings) = JsonSettingsStore::for_current_user() else {
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
