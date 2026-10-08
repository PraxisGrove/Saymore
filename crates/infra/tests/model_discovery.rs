use httpmock::{Method::GET, MockServer};
use template_infra::{ModelDiscoveryError, discover_models};

#[tokio::test]
async fn discovers_normalized_models_with_bearer_auth() -> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start_async().await;
    let models = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/v1/models")
                .header("authorization", "Bearer provider-key")
                .header("content-type", "application/json");
            then.status(200).json_body(serde_json::json!({
                "object": "list",
                "data": [
                    {"id": " model-b "},
                    {"id": "model-a"},
                    {"id": "model-a"},
                    {"id": ""}
                ]
            }));
        })
        .await;

    let discovered = discover_models(&server.url("/v1/models"), " provider-key ").await?;

    if discovered != vec!["model-a".to_owned(), "model-b".to_owned()] {
        return Err(format!("unexpected discovered models: {discovered:?}").into());
    }
    models.assert_async().await;
    Ok(())
}

#[tokio::test]
async fn classifies_model_discovery_http_failures() -> Result<(), Box<dyn std::error::Error>> {
    for status in [401_u16, 403, 429, 500] {
        let server = MockServer::start_async().await;
        let rejected = server
            .mock_async(|when, then| {
                when.method(GET).path("/models");
                then.status(status);
            })
            .await;

        let result = discover_models(&server.url("/models"), "provider-key").await;
        let classified = matches!(
            (status, result),
            (401 | 403, Err(ModelDiscoveryError::Authentication))
                | (429, Err(ModelDiscoveryError::RateLimited))
                | (500, Err(ModelDiscoveryError::Protocol(_)))
        );
        if !classified {
            return Err(format!("HTTP {status} was not classified correctly").into());
        }
        rejected.assert_async().await;
    }
    Ok(())
}

#[tokio::test]
async fn rejects_empty_and_malformed_model_lists() -> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start_async().await;
    let empty = server
        .mock_async(|when, then| {
            when.method(GET).path("/empty");
            then.status(200)
                .json_body(serde_json::json!({"data": [{"id": "  "}]}));
        })
        .await;
    let malformed = server
        .mock_async(|when, then| {
            when.method(GET).path("/malformed");
            then.status(200).body("not-json");
        })
        .await;

    if discover_models(&server.url("/empty"), "provider-key").await
        != Err(ModelDiscoveryError::Empty)
    {
        return Err("empty model list was not rejected".into());
    }
    if !matches!(
        discover_models(&server.url("/malformed"), "provider-key").await,
        Err(ModelDiscoveryError::Protocol(_))
    ) {
        return Err("malformed model list was not rejected as a protocol error".into());
    }
    empty.assert_async().await;
    malformed.assert_async().await;
    Ok(())
}

#[tokio::test]
async fn rejects_model_discovery_without_an_api_key() {
    assert_eq!(
        Err(ModelDiscoveryError::MissingApiKey),
        discover_models("https://unused.invalid/models", "  ").await
    );
}

#[tokio::test]
async fn discovers_models_from_a_keyless_loopback_service() -> Result<(), Box<dyn std::error::Error>>
{
    let server = MockServer::start_async().await;
    let models = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/v1/models")
                .header_missing("authorization");
            then.status(200)
                .json_body(serde_json::json!({"data": [{"id": "local-model"}]}));
        })
        .await;
    let discovered = discover_models(&server.url("/v1/models"), " ").await?;
    if discovered != vec!["local-model"] {
        return Err(format!("unexpected local models: {discovered:?}").into());
    }
    models.assert_async().await;
    Ok(())
}

#[tokio::test]
async fn rejects_invalid_discovery_urls_before_sending_credentials() {
    for endpoint in [
        "not-a-url",
        "file:///models",
        "https://user:password@provider.example/models",
        "https://provider.example/models#fragment",
    ] {
        assert!(matches!(
            discover_models(endpoint, "provider-key").await,
            Err(ModelDiscoveryError::Protocol(_))
        ));
    }
}
