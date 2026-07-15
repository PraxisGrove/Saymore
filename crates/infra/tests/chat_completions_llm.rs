use std::collections::BTreeMap;
#[cfg(target_os = "macos")]
use std::sync::Arc;
#[cfg(target_os = "macos")]
use std::time::{Duration, Instant};

use httpmock::{Method::POST, MockServer};
use template_app::{
    ChatCompletionsLlmSettings, LlmProvider, LlmProviderError, LlmRefinementRequest,
};
#[cfg(target_os = "macos")]
use template_app::{
    FinalTextProcessor, FinalTextRequest, RefinementEvaluationMode, RefinementMode,
    RefinementStatus,
};
#[cfg(target_os = "macos")]
use template_app::{ProviderConfigStore, SettingsStore};
use template_infra::ChatCompletionsLlmProvider;
#[cfg(target_os = "macos")]
use template_infra::{AppEnvironment, JsonSettingsStore};

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
                .body_includes(r#""max_tokens":44"#)
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
async fn sends_deepseek_v4_in_non_thinking_mode() -> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start_async().await;
    let completion = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/chat/completions")
                .body_includes(r#""model":"deepseek-v4-flash""#)
                .body_includes(r#""thinking":{"type":"disabled"}"#)
                .body_excludes("reasoning_effort");
            then.status(200).json_body(serde_json::json!({
                "choices": [{"message": {"content": "Refined text."}}]
            }));
        })
        .await;
    let provider = ChatCompletionsLlmProvider::new(ChatCompletionsLlmSettings {
        base_url: server.base_url(),
        api_key: "deepseek-key".to_owned(),
        model: "deepseek-v4-flash".to_owned(),
        custom_headers: BTreeMap::new(),
    })?;

    provider.test_connection().await?;

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
    let store = JsonSettingsStore::for_current_user(AppEnvironment::Production)?;
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
    #[derive(Clone, serde::Deserialize)]
    struct GoldenCase {
        id: String,
        transcript: String,
        expected: String,
        #[serde(default)]
        acceptable: Vec<String>,
        #[serde(default)]
        language: Option<String>,
        #[serde(default)]
        relevant_terms: Vec<GoldenTerm>,
    }

    #[derive(Clone, serde::Deserialize)]
    struct GoldenTerm {
        canonical: String,
    }

    let fixture = match std::env::var("SAYMORE_LLM_SUITE").as_deref() {
        Ok("word_order") => include_str!("fixtures/llm_word_order_cases.json"),
        Ok("general") | Err(_) => include_str!("fixtures/llm_refinement_cases.json"),
        Ok(other) => return Err(format!("unsupported SAYMORE_LLM_SUITE: {other}").into()),
    };
    let cases: Vec<GoldenCase> = serde_json::from_str(fixture)?;
    let case_filter = comma_separated_filter("SAYMORE_LLM_CASE");
    let provider_filter = comma_separated_filter("SAYMORE_LLM_PROVIDER");
    let environment = match std::env::var("SAYMORE_LLM_ENVIRONMENT").as_deref() {
        Ok("development") => AppEnvironment::Development,
        Ok("production") | Err(_) => AppEnvironment::Production,
        Ok(other) => return Err(format!("unsupported SAYMORE_LLM_ENVIRONMENT: {other}").into()),
    };
    let store = JsonSettingsStore::for_current_user(environment)?;
    let catalog = store.load_catalog()?;
    let providers = catalog
        .llm_providers
        .into_iter()
        .filter(|provider| {
            provider_filter.is_empty() || provider_filter.iter().any(|id| id == &provider.id)
        })
        .collect::<Vec<_>>();
    if providers.is_empty() {
        return Err("SAYMORE_LLM_PROVIDER did not match a configured provider".into());
    }
    let mut matched = false;
    let mut failures = Vec::new();

    for configured in providers {
        let settings = provider_settings(&configured)?;
        let provider = ChatCompletionsLlmProvider::new(settings)?;
        let processor = FinalTextProcessor::configured(Arc::new(provider));
        let mut durations = Vec::new();
        for case in cases
            .iter()
            .filter(|case| case_filter.is_empty() || case_filter.iter().any(|id| id == &case.id))
        {
            matched = true;
            let mut request =
                FinalTextRequest::new(case.transcript.clone(), RefinementMode::Enabled);
            request.language.clone_from(&case.language);
            request.relevant_terms = case
                .relevant_terms
                .iter()
                .map(|term| template_app::RefinementTerm {
                    canonical: term.canonical.clone(),
                })
                .collect();
            let started = Instant::now();
            let evaluation = processor
                .evaluate(
                    request,
                    tokio_util::sync::CancellationToken::new(),
                    RefinementEvaluationMode::ForceProvider,
                )
                .await?;
            durations.push(started.elapsed());
            let provider_output = evaluation.provider_output;
            let result = evaluation.processed;
            let text_matches = result.text == case.expected
                || case.acceptable.iter().any(|text| text == &result.text);
            if result.refinement != RefinementStatus::Completed || !text_matches {
                failures.push(format!(
                    "provider '{}' case '{}' differed\nstatus: {:?}\nexpected: {:?}\nacceptable: {:?}\nactual: {:?}\nprovider output: {:?}",
                    configured.name,
                    case.id,
                    result.refinement,
                    case.expected,
                    case.acceptable,
                    result.text,
                    provider_output
                ));
            }
        }
        let (average, p50, p95) = duration_summary(&mut durations);
        eprintln!(
            "prompt=v3 provider={} cases={} average_ms={} p50_ms={} p95_ms={}",
            configured.name,
            durations.len(),
            average.as_millis(),
            p50.as_millis(),
            p95.as_millis(),
        );
    }
    if !matched {
        return Err("SAYMORE_LLM_CASE did not match a golden case".into());
    }
    if !failures.is_empty() {
        return Err(failures.join("\n\n").into());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn duration_summary(durations: &mut [Duration]) -> (Duration, Duration, Duration) {
    if durations.is_empty() {
        return (Duration::ZERO, Duration::ZERO, Duration::ZERO);
    }
    durations.sort_unstable();
    let total = durations.iter().copied().sum::<Duration>();
    let count = u32::try_from(durations.len()).unwrap_or(u32::MAX);
    let percentile = |numerator: usize| {
        let index = durations
            .len()
            .saturating_mul(numerator)
            .div_ceil(100)
            .saturating_sub(1)
            .min(durations.len() - 1);
        durations[index]
    };
    (total / count, percentile(50), percentile(95))
}

#[cfg(target_os = "macos")]
fn comma_separated_filter(name: &str) -> Vec<String> {
    std::env::var(name)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

#[cfg(target_os = "macos")]
fn provider_settings(
    provider: &template_app::ProviderInstance,
) -> Result<ChatCompletionsLlmSettings, Box<dyn std::error::Error>> {
    let config = &provider.config;
    let required = |name: &str| {
        config
            .get(name)
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or_else(|| format!("provider '{}' is missing {name}", provider.name))
    };
    Ok(ChatCompletionsLlmSettings {
        base_url: required("base_url")?,
        api_key: required("api_key")?,
        model: required("model")?,
        custom_headers: config
            .get("custom_headers")
            .cloned()
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default(),
    })
}
