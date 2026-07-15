use std::time::Duration;

use reqwest::{
    Client, StatusCode,
    header::{CONTENT_TYPE, HeaderValue},
    redirect::Policy,
};
use serde::Deserialize;
use thiserror::Error;

const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ModelDiscoveryError {
    #[error("model discovery requires an API key")]
    MissingApiKey,
    #[error("model discovery authentication failed")]
    Authentication,
    #[error("model discovery was rate limited")]
    RateLimited,
    #[error("model discovery returned no models")]
    Empty,
    #[error("model discovery request failed: {0}")]
    Transport(String),
    #[error("model discovery response is invalid: {0}")]
    Protocol(String),
}

#[derive(Deserialize)]
struct ModelListResponse {
    data: Vec<ModelRecord>,
}

#[derive(Deserialize)]
struct ModelRecord {
    id: String,
}

pub async fn discover_models(
    endpoint: &str,
    api_key: &str,
) -> Result<Vec<String>, ModelDiscoveryError> {
    let api_key = api_key.trim();
    if api_key.is_empty() {
        return Err(ModelDiscoveryError::MissingApiKey);
    }
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = Client::builder()
        .redirect(Policy::none())
        .connect_timeout(DISCOVERY_TIMEOUT)
        .timeout(DISCOVERY_TIMEOUT)
        .build()
        .map_err(transport_error)?;
    let response = client
        .get(endpoint)
        .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .bearer_auth(api_key)
        .send()
        .await
        .map_err(transport_error)?;
    match response.status() {
        status if status.is_success() => {}
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            return Err(ModelDiscoveryError::Authentication);
        }
        StatusCode::TOO_MANY_REQUESTS => return Err(ModelDiscoveryError::RateLimited),
        status => {
            return Err(ModelDiscoveryError::Protocol(format!(
                "provider returned HTTP {status}"
            )));
        }
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(ModelDiscoveryError::Protocol(
            "response exceeds 1 MiB".to_owned(),
        ));
    }
    let bytes = response.bytes().await.map_err(transport_error)?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(ModelDiscoveryError::Protocol(
            "response exceeds 1 MiB".to_owned(),
        ));
    }
    let response: ModelListResponse = serde_json::from_slice(&bytes)
        .map_err(|_| ModelDiscoveryError::Protocol("response is not a model list".to_owned()))?;
    let mut models = response
        .data
        .into_iter()
        .map(|model| model.id.trim().to_owned())
        .filter(|model| !model.is_empty())
        .collect::<Vec<_>>();
    models.sort_unstable();
    models.dedup();
    if models.is_empty() {
        return Err(ModelDiscoveryError::Empty);
    }
    Ok(models)
}

fn transport_error(error: impl std::fmt::Display) -> ModelDiscoveryError {
    ModelDiscoveryError::Transport(error.to_string())
}
