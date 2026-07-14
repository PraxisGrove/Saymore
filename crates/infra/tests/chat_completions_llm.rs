use std::collections::BTreeMap;
#[cfg(target_os = "macos")]
use std::sync::Arc;

use httpmock::{Method::POST, MockServer};
#[cfg(target_os = "macos")]
use template_app::SettingsStore;
use template_app::{
    ChatCompletionsLlmSettings, LlmProvider, LlmProviderError, LlmRefinementRequest,
};
#[cfg(target_os = "macos")]
use template_app::{FinalTextProcessor, FinalTextRequest, RefinementMode, RefinementStatus};
use template_infra::ChatCompletionsLlmProvider;
#[cfg(target_os = "macos")]
use template_infra::JsonSettingsStore;

#[tokio::test]
async fn sends_an_openai_compatible_chat_completion_request()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start_async().await;
    let completion = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v1/chat/completions")
                .header("authorization", "Bearer test-key")
                .header("x-tenant", "tenant-a")
                .body_includes("Refine conservatively.")
                .body_includes(r#""model":"vendor-model""#)
                .body_includes(r#""role":"system""#)
                .body_includes(r#""role":"user""#)
                .body_includes(r#""stream":false"#)
                .body_includes(r#""reasoning_effort":"none""#)
                .body_includes(r#""max_tokens":128"#)
                .body_includes(r#""temperature":0.2"#)
                .body_includes("raw text")
                .body_includes("Typeless");
            then.status(200).json_body(serde_json::json!({
                "choices": [{
                    "message": {"content": "Refined text."}
                }]
            }));
        })
        .await;
    let provider = ChatCompletionsLlmProvider::new(ChatCompletionsLlmSettings {
        base_url: server.url("/v1"),
        api_key: "test-key".to_owned(),
        model: "vendor-model".to_owned(),
        custom_headers: BTreeMap::from([("X-Tenant".to_owned(), "tenant-a".to_owned())]),
    })?;

    let result = provider
        .refine(LlmRefinementRequest {
            instructions: "Refine conservatively.".to_owned(),
            transcript: "raw text".to_owned(),
            language: Some("zh-CN".to_owned()),
            relevant_terms: vec![template_app::RefinementTerm {
                canonical: "Typeless".to_owned(),
                recognized_as: vec!["table".to_owned()],
            }],
        })
        .await?;

    if result != "Refined text." {
        return Err("provider returned an unexpected completion".into());
    }
    completion.assert_async().await;
    Ok(())
}

#[tokio::test]
async fn accepts_a_full_chat_completions_endpoint() -> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start_async().await;
    let completion = server
        .mock_async(|when, then| {
            when.method(POST).path("/v1/chat/completions");
            then.status(200).json_body(serde_json::json!({
                "choices": [{"message": {"content": "ok"}}]
            }));
        })
        .await;
    let provider = ChatCompletionsLlmProvider::new(ChatCompletionsLlmSettings {
        base_url: server.url("/v1/chat/completions"),
        api_key: String::new(),
        model: "local-model".to_owned(),
        custom_headers: BTreeMap::new(),
    })?;

    provider.test_connection().await?;
    completion.assert_async().await;
    Ok(())
}

#[tokio::test]
async fn maps_a_bad_request_to_permanent_configuration_failure()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start_async().await;
    let rejected = server
        .mock_async(|when, then| {
            when.method(POST).path("/v1/chat/completions");
            then.status(400);
        })
        .await;
    let provider = ChatCompletionsLlmProvider::new(ChatCompletionsLlmSettings {
        base_url: server.url("/v1"),
        api_key: String::new(),
        model: "missing-model".to_owned(),
        custom_headers: BTreeMap::new(),
    })?;

    let result = provider.test_connection().await;

    if result != Err(LlmProviderError::InvalidConfiguration) {
        return Err("HTTP 400 was not classified as invalid configuration".into());
    }
    rejected.assert_async().await;
    Ok(())
}

#[cfg(target_os = "macos")]
#[tokio::test]
#[ignore = "requires a live LLM configuration in the current user's config file"]
async fn connects_using_current_user_llm_configuration() -> Result<(), Box<dyn std::error::Error>> {
    let store = JsonSettingsStore::for_current_user()?;
    let settings = store.load()?;
    let provider = ChatCompletionsLlmProvider::new(settings.llm.chat_completions)?;

    provider.test_connection().await?;
    Ok(())
}

#[cfg(target_os = "macos")]
#[tokio::test]
#[ignore = "sends synthetic golden cases to the live LLM in the current user's config file"]
async fn live_configuration_matches_refinement_golden_cases()
-> Result<(), Box<dyn std::error::Error>> {
    #[derive(serde::Deserialize)]
    struct GoldenCase {
        id: String,
        transcript: String,
        expected: String,
        #[serde(default)]
        language: Option<String>,
        #[serde(default)]
        relevant_terms: Vec<GoldenTerm>,
    }

    #[derive(serde::Deserialize)]
    struct GoldenTerm {
        canonical: String,
        recognized_as: Vec<String>,
    }

    let cases: Vec<GoldenCase> =
        serde_json::from_str(include_str!("fixtures/llm_refinement_cases.json"))?;
    let case_filter = std::env::var("SAYMORE_LLM_CASE").ok();
    let store = JsonSettingsStore::for_current_user()?;
    let settings = store.load()?;
    let provider = ChatCompletionsLlmProvider::new(settings.llm.chat_completions)?;
    let processor = FinalTextProcessor::configured(Arc::new(provider));
    let mut matched = false;

    for case in cases
        .into_iter()
        .filter(|case| case_filter.as_ref().is_none_or(|filter| filter == &case.id))
    {
        matched = true;
        let mut request = FinalTextRequest::new(case.transcript, RefinementMode::Enabled);
        request.language = case.language;
        request.relevant_terms = case
            .relevant_terms
            .into_iter()
            .map(|term| template_app::RefinementTerm {
                canonical: term.canonical,
                recognized_as: term.recognized_as,
            })
            .collect();
        let result = processor
            .process(request, tokio_util::sync::CancellationToken::new())
            .await?;
        if result.refinement != RefinementStatus::Completed {
            return Err(format!(
                "live refinement case '{}' did not complete: {:?}",
                case.id, result.refinement
            )
            .into());
        }
        if result.text != case.expected {
            return Err(format!(
                "live refinement case '{}' completed with different text",
                case.id
            )
            .into());
        }
    }
    if !matched {
        return Err("SAYMORE_LLM_CASE did not match a golden case".into());
    }
    Ok(())
}
