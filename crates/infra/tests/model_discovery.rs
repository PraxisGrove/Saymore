use httpmock::{Method::GET, MockServer};
use template_infra::discover_models;

#[tokio::test]
async fn discovers_deduplicated_models_with_bearer_authentication() {
    let server = MockServer::start_async().await;
    let request = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/models")
                .header("authorization", "Bearer test-key")
                .header("content-type", "application/json");
            then.status(200).json_body(serde_json::json!({
                "object": "list",
                "data": [
                    { "id": "model-b" },
                    { "id": "model-a" },
                    { "id": "model-a" },
                    { "id": " " }
                ]
            }));
        })
        .await;

    let result = discover_models(&format!("{}/models", server.base_url()), "test-key").await;

    request.assert_async().await;
    assert_eq!(Ok(vec!["model-a".to_owned(), "model-b".to_owned()]), result);
}

#[tokio::test]
async fn rejects_an_unauthorized_model_list_request() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(GET).path("/models");
            then.status(401);
        })
        .await;

    let result = discover_models(&format!("{}/models", server.base_url()), "bad-key").await;

    assert!(matches!(
        result,
        Err(template_infra::ModelDiscoveryError::Authentication)
    ));
}
