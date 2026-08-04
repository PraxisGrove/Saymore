use std::collections::BTreeMap;
#[cfg(target_os = "macos")]
use std::sync::Arc;
#[cfg(target_os = "macos")]
use std::time::{Duration, Instant};

use httpmock::{Method::POST, MockServer};
use template_app::{
    ChatCompletionsLlmSettings, ChatCompletionsProfile, LlmProvider, LlmProviderError,
    LlmProviderPreset, LlmRefinementRequest,
};
#[cfg(target_os = "macos")]
use template_app::{
    FinalTextProcessor, FinalTextRequest, RefinementEvaluationMode, RefinementMode,
    RefinementStatus,
};
#[cfg(target_os = "macos")]
use template_app::{ProviderConfigStore, SettingsStore};
#[cfg(target_os = "macos")]
use template_infra::{AppEnvironment, JsonSettingsStore};
use template_infra::{ChatCompletionsLlmProvider, discover_models};

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
                .body_includes(r#""model":"deepseek-proxy-model""#)
                .body_includes(r#""role":"system""#)
                .body_includes(r#""role":"user""#)
                .body_includes(r#""stream":false"#)
                .body_excludes("reasoning_effort")
                .body_excludes("thinking")
                .body_excludes("max_tokens")
                .body_excludes("max_completion_tokens")
                .body_excludes("temperature")
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
        model: "deepseek-proxy-model".to_owned(),
        custom_headers: BTreeMap::from([("X-Tenant".to_owned(), "tenant-a".to_owned())]),
        profile: ChatCompletionsProfile::Portable,
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
        profile: ChatCompletionsProfile::DeepSeek,
    })?;

    provider.test_connection().await?;

    completion.assert_async().await;
    Ok(())
}

#[tokio::test]
async fn sends_kimi_in_non_thinking_mode() -> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start_async().await;
    let completion = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v1/chat/completions")
                .body_includes(r#""model":"kimi-k3""#)
                .body_includes(r#""thinking":{"type":"disabled"}"#)
                .body_excludes("reasoning_effort")
                .body_excludes("max_tokens")
                .body_excludes("max_completion_tokens")
                .body_excludes("temperature");
            then.status(200).json_body(serde_json::json!({
                "choices": [{"message": {"content": "Refined text."}}]
            }));
        })
        .await;
    let mut settings = LlmProviderPreset::Kimi.settings("kimi-key");
    settings.base_url = server.url("/v1");
    settings.model = "kimi-k3".to_owned();
    let provider = ChatCompletionsLlmProvider::new(settings)?;

    provider.test_connection().await?;

    completion.assert_async().await;
    Ok(())
}

#[tokio::test]
async fn sends_minimal_requests_for_the_new_openai_compatible_profiles()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start_async().await;
    let completion = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v1/chat/completions")
                .body_excludes("reasoning_effort")
                .body_excludes("thinking")
                .body_excludes("max_tokens")
                .body_excludes("temperature");
            then.status(200).json_body(serde_json::json!({
                "choices": [{"message": {"content": "Refined text."}}]
            }));
        })
        .await;

    for profile in [
        ChatCompletionsProfile::VolcengineArk,
        ChatCompletionsProfile::Qwen,
        ChatCompletionsProfile::ZhipuGlm,
    ] {
        let provider = ChatCompletionsLlmProvider::new(ChatCompletionsLlmSettings {
            base_url: server.url("/v1"),
            api_key: "test-key".to_owned(),
            model: "provider-model".to_owned(),
            custom_headers: BTreeMap::new(),
            profile,
        })?;
        provider.test_connection().await?;
    }

    if completion.calls_async().await != 3 {
        return Err("each new provider profile should send one request".into());
    }
    Ok(())
}

#[tokio::test]
async fn siliconflow_and_stepfun_use_the_portable_chat_contract()
-> Result<(), Box<dyn std::error::Error>> {
    for preset in [LlmProviderPreset::SiliconFlow, LlmProviderPreset::StepFun] {
        let server = MockServer::start_async().await;
        let expected_model = format!(r#""model":"{}""#, preset.model());
        let completion = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/chat/completions")
                    .header("authorization", "Bearer provider-key")
                    .body_includes(expected_model.as_str())
                    .body_excludes("reasoning_effort")
                    .body_excludes("thinking")
                    .body_excludes("max_tokens")
                    .body_excludes("max_completion_tokens")
                    .body_excludes("temperature");
                then.status(200).json_body(serde_json::json!({
                    "choices": [{"message": {"content": "Refined text."}}]
                }));
            })
            .await;
        let mut settings = preset.settings("provider-key");
        settings.base_url = server.url("/v1");
        let provider = ChatCompletionsLlmProvider::new(settings)?;

        provider.test_connection().await?;

        completion.assert_async().await;
    }
    Ok(())
}

#[tokio::test]
async fn portable_payment_required_maps_to_quota() -> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start_async().await;
    let rejected = server
        .mock_async(|when, then| {
            when.method(POST).path("/v1/chat/completions");
            then.status(402).json_body(serde_json::json!({
                "error": {"message": "insufficient balance"}
            }));
        })
        .await;
    let mut settings = LlmProviderPreset::StepFun.settings("step-key");
    settings.base_url = server.url("/v1");
    let provider = ChatCompletionsLlmProvider::new(settings)?;

    if provider.test_connection().await != Err(LlmProviderError::Quota) {
        return Err("HTTP 402 was not classified as quota".into());
    }
    rejected.assert_async().await;
    Ok(())
}

#[tokio::test]
async fn separates_minimax_reasoning_from_refined_content() -> Result<(), Box<dyn std::error::Error>>
{
    let server = MockServer::start_async().await;
    let completion = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v1/chat/completions")
                .body_includes(r#""model":"MiniMax-M3""#)
                .body_includes(r#""reasoning_split":true"#)
                .body_includes(r#""thinking":{"type":"disabled"}"#)
                .body_excludes("temperature");
            then.status(200).json_body(serde_json::json!({
                "choices": [{"message": {"content": "Refined text."}}],
                "base_resp": {"status_code": 0, "status_msg": "success"}
            }));
        })
        .await;
    let provider = ChatCompletionsLlmProvider::new(ChatCompletionsLlmSettings {
        base_url: server.url("/v1"),
        api_key: "minimax-key".to_owned(),
        model: "MiniMax-M3".to_owned(),
        custom_headers: BTreeMap::new(),
        profile: ChatCompletionsProfile::MiniMax,
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
        profile: ChatCompletionsProfile::Portable,
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
        profile: ChatCompletionsProfile::Portable,
    })?;

    let result = provider.test_connection().await;

    if result != Err(LlmProviderError::InvalidConfiguration) {
        return Err("HTTP 400 was not classified as invalid configuration".into());
    }
    rejected.assert_async().await;
    Ok(())
}

#[tokio::test]
async fn classifies_common_chat_completion_http_failures() -> Result<(), Box<dyn std::error::Error>>
{
    for status in [401_u16, 404, 429, 503] {
        let server = MockServer::start_async().await;
        let rejected = server
            .mock_async(|when, then| {
                when.method(POST).path("/v1/chat/completions");
                then.status(status);
            })
            .await;
        let provider = ChatCompletionsLlmProvider::new(ChatCompletionsLlmSettings {
            base_url: server.url("/v1"),
            api_key: "test-key".to_owned(),
            model: "test-model".to_owned(),
            custom_headers: BTreeMap::new(),
            profile: ChatCompletionsProfile::Portable,
        })?;

        let result = provider.test_connection().await;
        let classified = matches!(
            (status, result),
            (401, Err(LlmProviderError::Authentication))
                | (404, Err(LlmProviderError::ModelUnavailable))
                | (429, Err(LlmProviderError::Quota))
                | (503, Err(LlmProviderError::Transport(_)))
        );
        if !classified {
            return Err(format!("HTTP {status} was not classified correctly").into());
        }
        rejected.assert_async().await;
    }
    Ok(())
}

#[tokio::test]
async fn rejects_success_responses_without_refined_text() -> Result<(), Box<dyn std::error::Error>>
{
    let server = MockServer::start_async().await;
    let empty = server
        .mock_async(|when, then| {
            when.method(POST).path("/v1/chat/completions");
            then.status(200).json_body(serde_json::json!({
                "choices": [{"message": {"content": "  "}}]
            }));
        })
        .await;
    let provider = ChatCompletionsLlmProvider::new(ChatCompletionsLlmSettings {
        base_url: server.url("/v1"),
        api_key: "test-key".to_owned(),
        model: "test-model".to_owned(),
        custom_headers: BTreeMap::new(),
        profile: ChatCompletionsProfile::Portable,
    })?;

    if !matches!(
        provider.test_connection().await,
        Err(LlmProviderError::Protocol(_))
    ) {
        return Err("empty completion content was accepted".into());
    }
    empty.assert_async().await;
    Ok(())
}

#[tokio::test]
async fn maps_openrouter_payment_required_to_quota() -> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start_async().await;
    let rejected = server
        .mock_async(|when, then| {
            when.method(POST).path("/v1/chat/completions");
            then.status(402);
        })
        .await;
    let provider = ChatCompletionsLlmProvider::new(ChatCompletionsLlmSettings {
        base_url: server.url("/v1"),
        api_key: "test-key".to_owned(),
        model: "openrouter/auto".to_owned(),
        custom_headers: BTreeMap::new(),
        profile: ChatCompletionsProfile::OpenRouter,
    })?;

    if provider.test_connection().await != Err(LlmProviderError::Quota) {
        return Err("OpenRouter HTTP 402 was not classified as quota".into());
    }
    rejected.assert_async().await;
    Ok(())
}

#[tokio::test]
async fn maps_qwen_arrearage_to_quota() -> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start_async().await;
    let rejected = server
        .mock_async(|when, then| {
            when.method(POST).path("/v1/chat/completions");
            then.status(400).json_body(serde_json::json!({
                "code": "Arrearage",
                "message": "account balance is overdue"
            }));
        })
        .await;
    let provider = ChatCompletionsLlmProvider::new(ChatCompletionsLlmSettings {
        base_url: server.url("/v1"),
        api_key: "test-key".to_owned(),
        model: "qwen-plus".to_owned(),
        custom_headers: BTreeMap::new(),
        profile: ChatCompletionsProfile::Qwen,
    })?;

    if provider.test_connection().await != Err(LlmProviderError::Quota) {
        return Err("Qwen Arrearage was not classified as quota".into());
    }
    rejected.assert_async().await;
    Ok(())
}

#[tokio::test]
async fn maps_zhipu_missing_model_code_to_model_unavailable()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start_async().await;
    let rejected = server
        .mock_async(|when, then| {
            when.method(POST).path("/v4/chat/completions");
            then.status(400).json_body(serde_json::json!({
                "error": {"code": 1211, "message": "model does not exist"}
            }));
        })
        .await;
    let provider = ChatCompletionsLlmProvider::new(ChatCompletionsLlmSettings {
        base_url: server.url("/v4"),
        api_key: "test-key".to_owned(),
        model: "missing-model".to_owned(),
        custom_headers: BTreeMap::new(),
        profile: ChatCompletionsProfile::ZhipuGlm,
    })?;

    if provider.test_connection().await != Err(LlmProviderError::ModelUnavailable) {
        return Err("Zhipu code 1211 was not classified as unavailable model".into());
    }
    rejected.assert_async().await;
    Ok(())
}

#[tokio::test]
async fn maps_minimax_business_error_from_successful_http_response()
-> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start_async().await;
    let rejected = server
        .mock_async(|when, then| {
            when.method(POST).path("/v1/chat/completions");
            then.status(200).json_body(serde_json::json!({
                "base_resp": {"status_code": 1008, "status_msg": "insufficient balance"}
            }));
        })
        .await;
    let provider = ChatCompletionsLlmProvider::new(ChatCompletionsLlmSettings {
        base_url: server.url("/v1"),
        api_key: "minimax-key".to_owned(),
        model: "MiniMax-M3".to_owned(),
        custom_headers: BTreeMap::new(),
        profile: ChatCompletionsProfile::MiniMax,
    })?;

    if provider.test_connection().await != Err(LlmProviderError::Quota) {
        return Err("MiniMax code 1008 was not classified as quota".into());
    }
    rejected.assert_async().await;
    Ok(())
}

#[tokio::test]
async fn maps_minimax_invalid_key_from_http_error() -> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start_async().await;
    let rejected = server
        .mock_async(|when, then| {
            when.method(POST).path("/v1/chat/completions");
            then.status(400).json_body(serde_json::json!({
                "base_resp": {"status_code": 2049, "status_msg": "invalid API key"}
            }));
        })
        .await;
    let provider = ChatCompletionsLlmProvider::new(ChatCompletionsLlmSettings {
        base_url: server.url("/v1"),
        api_key: "invalid-key".to_owned(),
        model: "MiniMax-M3".to_owned(),
        custom_headers: BTreeMap::new(),
        profile: ChatCompletionsProfile::MiniMax,
    })?;

    if provider.test_connection().await != Err(LlmProviderError::Authentication) {
        return Err("MiniMax code 2049 was not classified as authentication".into());
    }
    rejected.assert_async().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires SAYMORE_LLM_SMOKE_PROVIDER and that provider's API key environment variable"]
async fn live_provider_smoke_test_from_environment() -> Result<(), Box<dyn std::error::Error>> {
    let provider_id = std::env::var("SAYMORE_LLM_SMOKE_PROVIDER")?;
    let preset = LlmProviderPreset::ALL
        .into_iter()
        .find(|preset| preset.id() == provider_id && *preset != LlmProviderPreset::Custom)
        .ok_or("SAYMORE_LLM_SMOKE_PROVIDER must name a built-in provider")?;
    let profile = preset.profile();
    let api_key = std::env::var(profile.api_key_environment)?;
    let models = if preset.supports_remote_model_discovery() {
        discover_models(profile.model_list_url, &api_key).await?
    } else {
        preset
            .recommended_models()
            .iter()
            .map(ToString::to_string)
            .collect()
    };
    if models.is_empty() {
        return Err("provider returned an empty model list".into());
    }
    let mut settings = preset.settings(&api_key);
    if let Ok(model) = std::env::var("SAYMORE_LLM_SMOKE_MODEL") {
        settings.model = model;
    }

    ChatCompletionsLlmProvider::new(settings)?
        .test_connection()
        .await?;
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
        profile: LlmProviderPreset::from_id_or_base_url(&provider.id, &required("base_url")?)
            .map(|preset| preset.profile().chat_completions)
            .unwrap_or_default(),
    })
}
