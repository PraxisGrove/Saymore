use std::time::Duration;

use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::{
    Client, StatusCode, Url,
    header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue},
    redirect::Policy,
};
use serde::{Deserialize, Serialize};
use template_app::{
    ChatCompletionsLlmSettings, LlmProvider, LlmProviderError, LlmRefinementRequest, RefinementTerm,
};

const MAX_REQUEST_BYTES: usize = 256 * 1024;
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const MIN_COMPLETION_TOKENS: u32 = 32;
const MAX_COMPLETION_TOKENS: u32 = 8_192;
const PROVIDER_TIMEOUT: Duration = Duration::from_secs(8);

pub struct ChatCompletionsLlmProvider {
    client: Client,
    endpoint: Url,
    api_key: String,
    model: String,
    custom_headers: HeaderMap,
}

impl ChatCompletionsLlmProvider {
    pub fn new(settings: ChatCompletionsLlmSettings) -> Result<Self, LlmProviderError> {
        Self::with_timeout(settings, PROVIDER_TIMEOUT)
    }

    fn with_timeout(
        settings: ChatCompletionsLlmSettings,
        timeout: Duration,
    ) -> Result<Self, LlmProviderError> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let endpoint = completion_endpoint(&settings.base_url)?;
        if settings.model.trim().is_empty() {
            return Err(LlmProviderError::ModelUnavailable);
        }
        let custom_headers =
            parse_headers(settings.custom_headers, settings.api_key.trim().is_empty())?;
        let client = Client::builder()
            .redirect(Policy::none())
            .connect_timeout(timeout)
            .timeout(timeout)
            .build()
            .map_err(transport_error)?;
        Ok(Self {
            client,
            endpoint,
            api_key: settings.api_key,
            model: settings.model,
            custom_headers,
        })
    }

    pub async fn test_connection(&self) -> Result<(), LlmProviderError> {
        self.refine(LlmRefinementRequest {
            instructions: "Return a short plain-text response.".to_owned(),
            transcript: "Connection test.".to_owned(),
            language: None,
            relevant_terms: Vec::new(),
        })
        .await
        .map(|_| ())
    }

    async fn request(&self, request: LlmRefinementRequest) -> Result<String, LlmProviderError> {
        let user_content = refinement_content(&request)?;
        if request
            .instructions
            .len()
            .saturating_add(user_content.len())
            > MAX_REQUEST_BYTES
        {
            return Err(LlmProviderError::Protocol(
                "LLM refinement request is too large".to_owned(),
            ));
        }
        let body = ChatCompletionRequest {
            model: &self.model,
            messages: [
                ChatMessage {
                    role: "system",
                    content: &request.instructions,
                },
                ChatMessage {
                    role: "user",
                    content: &user_content,
                },
            ],
            stream: false,
            reasoning_effort: "none",
            max_tokens: completion_token_limit(&request.transcript),
            temperature: 0.2,
        };
        let mut builder = self
            .client
            .post(self.endpoint.clone())
            .headers(self.custom_headers.clone())
            .json(&body);
        if !self.api_key.trim().is_empty() {
            builder = builder.bearer_auth(&self.api_key);
        }
        let response = builder.send().await.map_err(transport_error)?;
        let status = response.status();
        if !status.is_success() {
            return Err(http_error(status));
        }
        let bytes = read_limited_response(response).await?;
        let completion: ChatCompletionResponse = serde_json::from_slice(&bytes)
            .map_err(|_| protocol_failure("chat completion response is invalid JSON"))?;
        completion
            .choices
            .into_iter()
            .next()
            .and_then(|choice| choice.message.content)
            .filter(|content| !content.trim().is_empty())
            .ok_or_else(|| {
                LlmProviderError::Protocol(
                    "chat completion response has no text content".to_owned(),
                )
            })
    }
}

#[async_trait]
impl LlmProvider for ChatCompletionsLlmProvider {
    async fn refine(&self, request: LlmRefinementRequest) -> Result<String, LlmProviderError> {
        self.request(request).await
    }
}

#[derive(Serialize)]
struct ChatCompletionRequest<'a> {
    model: &'a str,
    messages: [ChatMessage<'a>; 2],
    stream: bool,
    reasoning_effort: &'static str,
    max_tokens: u32,
    temperature: f32,
}

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'static str,
    content: &'a str,
}

#[derive(Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatResponseMessage,
}

#[derive(Deserialize)]
struct ChatResponseMessage {
    content: Option<String>,
}

#[derive(Serialize)]
struct RefinementContent<'a> {
    transcript: &'a str,
    language: Option<&'a str>,
    relevant_terms: Vec<RefinementTermContent<'a>>,
}

#[derive(Serialize)]
struct RefinementTermContent<'a> {
    canonical: &'a str,
    recognized_as: &'a [String],
}

fn refinement_content(request: &LlmRefinementRequest) -> Result<String, LlmProviderError> {
    let relevant_terms = request
        .relevant_terms
        .iter()
        .map(|term: &RefinementTerm| RefinementTermContent {
            canonical: &term.canonical,
            recognized_as: &term.recognized_as,
        })
        .collect();
    serde_json::to_string(&RefinementContent {
        transcript: &request.transcript,
        language: request.language.as_deref(),
        relevant_terms,
    })
    .map_err(|_| protocol_failure("LLM refinement content could not be encoded"))
}

fn completion_token_limit(transcript: &str) -> u32 {
    let source_chars = match u32::try_from(transcript.chars().count()) {
        Ok(count) => count,
        Err(_) => MAX_COMPLETION_TOKENS,
    };
    source_chars
        .saturating_mul(3)
        .div_ceil(2)
        .saturating_add(32)
        .clamp(MIN_COMPLETION_TOKENS, MAX_COMPLETION_TOKENS)
}

fn completion_endpoint(base_url: &str) -> Result<Url, LlmProviderError> {
    let base_url = base_url.trim().trim_end_matches('/');
    if base_url.is_empty() {
        return Err(LlmProviderError::InvalidConfiguration);
    }
    let endpoint = if base_url.ends_with("/chat/completions") {
        base_url.to_owned()
    } else {
        format!("{base_url}/chat/completions")
    };
    let url = Url::parse(&endpoint).map_err(|_| LlmProviderError::InvalidConfiguration)?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(LlmProviderError::InvalidConfiguration);
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(LlmProviderError::InvalidConfiguration);
    }
    Ok(url)
}

fn parse_headers(
    headers: std::collections::BTreeMap<String, String>,
    allow_authorization: bool,
) -> Result<HeaderMap, LlmProviderError> {
    let mut parsed = HeaderMap::new();
    for (name, value) in headers {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| LlmProviderError::InvalidConfiguration)?;
        if name == CONTENT_TYPE || (name == AUTHORIZATION && !allow_authorization) {
            return Err(LlmProviderError::InvalidConfiguration);
        }
        let value =
            HeaderValue::from_str(&value).map_err(|_| LlmProviderError::InvalidConfiguration)?;
        parsed.insert(name, value);
    }
    Ok(parsed)
}

async fn read_limited_response(response: reqwest::Response) -> Result<Vec<u8>, LlmProviderError> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(LlmProviderError::Protocol(
            "chat completion response is too large".to_owned(),
        ));
    }
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(transport_error)?;
        if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err(LlmProviderError::Protocol(
                "chat completion response is too large".to_owned(),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn http_error(status: StatusCode) -> LlmProviderError {
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => LlmProviderError::Authentication,
        StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY => {
            LlmProviderError::InvalidConfiguration
        }
        StatusCode::NOT_FOUND => LlmProviderError::ModelUnavailable,
        StatusCode::TOO_MANY_REQUESTS => LlmProviderError::Quota,
        status if status.is_server_error() => {
            LlmProviderError::Transport(format!("LLM endpoint returned HTTP {status}"))
        }
        status => LlmProviderError::Protocol(format!(
            "LLM endpoint rejected the request with HTTP {status}"
        )),
    }
}

fn transport_error(_error: impl std::fmt::Display) -> LlmProviderError {
    LlmProviderError::Transport("LLM request failed".to_owned())
}

fn protocol_failure(message: &str) -> LlmProviderError {
    LlmProviderError::Protocol(message.to_owned())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use httpmock::{Method::POST, MockServer};

    use super::*;

    #[test]
    fn completion_budget_scales_with_transcript_and_stays_bounded() {
        assert_eq!(
            (40, 1_532, 8_192),
            (
                completion_token_limit("short"),
                completion_token_limit(&"字".repeat(1_000)),
                completion_token_limit(&"字".repeat(10_000)),
            )
        );
    }

    #[tokio::test]
    async fn connection_test_honors_provider_timeout() -> Result<(), Box<dyn std::error::Error>> {
        let server = MockServer::start_async().await;
        let _slow = server
            .mock_async(|when, then| {
                when.method(POST).path("/v1/chat/completions");
                then.delay(Duration::from_millis(100)).status(200);
            })
            .await;
        let provider = ChatCompletionsLlmProvider::with_timeout(
            ChatCompletionsLlmSettings {
                base_url: server.url("/v1"),
                api_key: String::new(),
                model: "test-model".to_owned(),
                custom_headers: BTreeMap::new(),
            },
            Duration::from_millis(5),
        )?;

        if !matches!(
            provider.test_connection().await,
            Err(LlmProviderError::Transport(_))
        ) {
            return Err("connection test did not honor the provider timeout".into());
        }
        Ok(())
    }
}
