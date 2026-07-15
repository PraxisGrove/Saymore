use httpmock::{Method::POST, MockServer};
use template_app::{OpenAiCompatibleAsrSettings, SpeechRecognitionError};
use template_infra::OpenAiCompatibleSpeechRecognizer;

#[tokio::test]
async fn sends_an_openai_compatible_transcription_request() -> Result<(), Box<dyn std::error::Error>>
{
    let server = MockServer::start_async().await;
    let transcription = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v1/audio/transcriptions")
                .header("authorization", "Bearer test-key")
                .body_includes("name=\"model\"")
                .body_includes("vendor-asr")
                .body_includes("filename=\"saymore.wav\"");
            then.status(200)
                .json_body(serde_json::json!({ "text": "" }));
        })
        .await;
    let recognizer = OpenAiCompatibleSpeechRecognizer::new(OpenAiCompatibleAsrSettings {
        enabled: true,
        base_url: server.url("/v1"),
        api_key: "test-key".to_owned(),
        model: "vendor-asr".to_owned(),
    })?;

    recognizer.test_connection().await?;

    transcription.assert_async().await;
    Ok(())
}

#[tokio::test]
async fn maps_transcription_authentication_failures() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(POST).path("/audio/transcriptions");
            then.status(401);
        })
        .await;
    let recognizer = OpenAiCompatibleSpeechRecognizer::new(OpenAiCompatibleAsrSettings {
        enabled: true,
        base_url: server.base_url(),
        api_key: "bad-key".to_owned(),
        model: "vendor-asr".to_owned(),
    });

    assert!(recognizer.is_ok());
    if let Ok(recognizer) = recognizer {
        assert_eq!(
            Err(SpeechRecognitionError::Authentication),
            recognizer.test_connection().await
        );
    }
}
