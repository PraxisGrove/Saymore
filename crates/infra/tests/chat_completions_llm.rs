use std::collections::BTreeMap;

use httpmock::{Method::POST, MockServer};
#[cfg(target_os = "macos")]
use template_app::SettingsStore;
use template_app::{
    ChatCompletionsLlmSettings, LlmProvider, LlmProviderError, LlmRefinementRequest,
};
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
                .json_body(serde_json::json!({
                    "model": "vendor-model",
                    "messages": [
                        {"role": "system", "content": "Refine conservatively."},
                        {"role": "user", "content": "{\"transcript\":\"raw text\",\"language\":\"zh-CN\",\"relevant_terms\":[{\"canonical\":\"Typeless\",\"recognized_as\":[\"table\"]}]}"}
                    ],
                    "stream": false
                }));
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
