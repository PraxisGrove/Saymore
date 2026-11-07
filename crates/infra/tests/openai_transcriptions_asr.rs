#![allow(clippy::panic_in_result_fn)]

use std::sync::Arc;

use httpmock::{Method::POST, MockServer};
use template_app::{
    OpenAiCompatibleAsrSettings, SpeechRecognitionError, SpeechRecognitionHints,
    StreamingSpeechRecognizer,
};
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
                .json_body(serde_json::json!({ "text": "standard test result" }));
        })
        .await;
    let recognizer = OpenAiCompatibleSpeechRecognizer::new(OpenAiCompatibleAsrSettings {
        enabled: true,
        base_url: server.url("/v1"),
        api_key: "test-key".to_owned(),
        model: "vendor-asr".to_owned(),
    })?;

    recognizer.test_audio(vec![1; 1_600]).await?;

    transcription.assert_async().await;
    Ok(())
}

#[tokio::test]
async fn rejects_an_empty_recognition_test_result() -> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(POST).path("/audio/transcriptions");
            then.status(200)
                .json_body(serde_json::json!({ "text": "" }));
        })
        .await;
    let recognizer = OpenAiCompatibleSpeechRecognizer::new(OpenAiCompatibleAsrSettings {
        enabled: true,
        base_url: server.base_url(),
        api_key: "test-key".to_owned(),
        model: "vendor-asr".to_owned(),
    })?;

    match recognizer.test_audio(vec![1; 1_600]).await {
        Err(SpeechRecognitionError::Protocol(_)) => Ok(()),
        result => Err(format!("expected an empty-result protocol error, got {result:?}").into()),
    }
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
            recognizer.test_audio(vec![1; 1_600]).await
        );
    }
}

#[test]
fn sends_dictionary_terms_as_the_transcription_prompt() -> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start();
    let transcription = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/audio/transcriptions")
            .body_includes("name=\"prompt\"")
            .body_includes("Saymore, DeepSeek");
        then.status(200)
            .json_body(serde_json::json!({ "text": "Saymore 接入 DeepSeek" }));
    });
    let recognizer = OpenAiCompatibleSpeechRecognizer::new(OpenAiCompatibleAsrSettings {
        enabled: true,
        base_url: server.url("/v1"),
        api_key: "asr-key".to_owned(),
        model: "vendor-asr".to_owned(),
    })?;
    let hints = SpeechRecognitionHints::from_terms(vec![
        "Saymore".to_owned(),
        " ".to_owned(),
        "saymore".to_owned(),
        "DeepSeek".to_owned(),
    ]);

    let session = recognizer.start(hints, Arc::new(|_| {}))?;
    session.push_audio(vec![1; 1_600])?;

    assert_eq!("Saymore 接入 DeepSeek", session.finish()?);
    transcription.assert();
    Ok(())
}

#[test]
fn omits_the_prompt_without_dictionary_terms() -> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start();
    let transcription = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/audio/transcriptions")
            .body_excludes("name=\"prompt\"");
        then.status(200)
            .json_body(serde_json::json!({ "text": "plain result" }));
    });
    let recognizer = OpenAiCompatibleSpeechRecognizer::new(OpenAiCompatibleAsrSettings {
        enabled: true,
        base_url: server.url("/v1"),
        api_key: "asr-key".to_owned(),
        model: "vendor-asr".to_owned(),
    })?;

    let session = recognizer.start(SpeechRecognitionHints::default(), Arc::new(|_| {}))?;
    session.push_audio(vec![1; 1_600])?;

    assert_eq!("plain result", session.finish()?);
    transcription.assert();
    Ok(())
}
