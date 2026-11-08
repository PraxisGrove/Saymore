use std::{
    io::Write,
    sync::{Arc, Mutex},
    time::Duration,
};

use reqwest::{
    Client, StatusCode, Url,
    multipart::{Form, Part},
    redirect::Policy,
};
use serde::Deserialize;
use template_app::{
    CloudAsrPreset, OpenAiCompatibleAsrSettings, SpeechRecognitionError, SpeechRecognitionHints,
    StreamingRecognitionSession, StreamingSpeechRecognizer,
};

const SAMPLE_RATE: u32 = 16_000;
const MAX_SAMPLES: usize = SAMPLE_RATE as usize * 60 * 30;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(90);
const RECOGNITION_TEST_TIMEOUT: Duration = Duration::from_secs(12);
const MAX_PROMPT_TERMS: usize = 64;
const MAX_PROMPT_CHARS: usize = 600;

pub struct OpenAiCompatibleSpeechRecognizer {
    client: Client,
    endpoint: Url,
    api_key: String,
    model: String,
    vocabulary: VocabularyHints,
    limits: TranscriptionLimits,
}

impl OpenAiCompatibleSpeechRecognizer {
    pub fn new(settings: OpenAiCompatibleAsrSettings) -> Result<Self, SpeechRecognitionError> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let endpoint = transcription_endpoint(&settings.base_url)?;
        if settings.api_key.trim().is_empty() || settings.model.trim().is_empty() {
            return Err(SpeechRecognitionError::NotConfigured);
        }
        let client = Client::builder()
            .redirect(Policy::none())
            .connect_timeout(Duration::from_secs(8))
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(transport_error)?;
        Ok(Self {
            client,
            endpoint,
            api_key: settings.api_key.trim().to_owned(),
            model: settings.model.trim().to_owned(),
            vocabulary: VocabularyHints::Prompt(None),
            limits: TranscriptionLimits::for_endpoint(&settings.base_url),
        })
    }

    pub async fn test_audio(&self, samples: Vec<i16>) -> Result<(), SpeechRecognitionError> {
        tokio::time::timeout(RECOGNITION_TEST_TIMEOUT, self.transcribe(samples, false))
            .await
            .map_err(|_| SpeechRecognitionError::Timeout)??;
        Ok(())
    }

    async fn transcribe(
        &self,
        samples: Vec<i16>,
        allow_empty: bool,
    ) -> Result<String, SpeechRecognitionError> {
        self.limits.validate_audio(samples.len())?;
        let audio = Part::bytes(wav_bytes(&samples)?)
            .file_name("saymore.wav")
            .mime_str("audio/wav")
            .map_err(protocol_error)?;
        let mut form = Form::new().text("model", self.model.clone());
        match &self.vocabulary {
            VocabularyHints::Prompt(Some(prompt)) => form = form.text("prompt", prompt.clone()),
            VocabularyHints::Prompt(None) => {}
            VocabularyHints::ContextBias(terms) => {
                for term in terms {
                    form = form.text("context_bias", term.clone());
                }
            }
        }
        let response = self
            .client
            .post(self.endpoint.clone())
            .bearer_auth(&self.api_key)
            .multipart(form.part("file", audio))
            .send()
            .await
            .map_err(transport_error)?;
        let status = response.status();
        if !status.is_success() {
            return Err(http_error(status));
        }
        let transcript: TranscriptionResponse = response.json().await.map_err(protocol_error)?;
        let text = transcript.text.trim().to_owned();
        if text.is_empty() && !allow_empty {
            return Err(SpeechRecognitionError::Protocol(
                "transcription response contains no text".to_owned(),
            ));
        }
        Ok(text)
    }
}

impl StreamingSpeechRecognizer for OpenAiCompatibleSpeechRecognizer {
    fn start(
        &self,
        hints: SpeechRecognitionHints,
        _on_partial: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Result<Box<dyn StreamingRecognitionSession>, SpeechRecognitionError> {
        Ok(Box::new(OpenAiCompatibleSession {
            client: self.client.clone(),
            endpoint: self.endpoint.clone(),
            api_key: self.api_key.clone(),
            model: self.model.clone(),
            vocabulary: vocabulary_hints(&hints, self.limits),
            limits: self.limits,
            samples: Mutex::new(Vec::new()),
        }))
    }
}

struct OpenAiCompatibleSession {
    client: Client,
    endpoint: Url,
    api_key: String,
    model: String,
    vocabulary: VocabularyHints,
    limits: TranscriptionLimits,
    samples: Mutex<Vec<i16>>,
}

impl StreamingRecognitionSession for OpenAiCompatibleSession {
    fn push_audio(&self, samples: Vec<i16>) -> Result<(), SpeechRecognitionError> {
        let mut buffered = self.samples.lock().map_err(|_| {
            SpeechRecognitionError::Transport("ASR audio lock was poisoned".to_owned())
        })?;
        self.limits
            .validate_audio(buffered.len().saturating_add(samples.len()))?;
        buffered.extend(samples);
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<String, SpeechRecognitionError> {
        let samples = self.samples.into_inner().map_err(|_| {
            SpeechRecognitionError::Transport("ASR audio lock was poisoned".to_owned())
        })?;
        let recognizer = OpenAiCompatibleSpeechRecognizer {
            client: self.client,
            endpoint: self.endpoint,
            api_key: self.api_key,
            model: self.model,
            vocabulary: self.vocabulary,
            limits: self.limits,
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(transport_error)?;
        runtime.block_on(recognizer.transcribe(samples, false))
    }

    fn cancel(self: Box<Self>) {}
}

#[derive(Clone, Copy)]
struct TranscriptionLimits {
    preset: Option<CloudAsrPreset>,
    max_samples: usize,
    max_prompt_bytes: Option<usize>,
}

impl TranscriptionLimits {
    fn for_endpoint(base_url: &str) -> Self {
        let preset = CloudAsrPreset::from_base_url(base_url);
        let max_samples = match preset {
            Some(CloudAsrPreset::OpenAi | CloudAsrPreset::Groq) => (25_000_000 - 44) / 2,
            _ => MAX_SAMPLES,
        };
        Self {
            preset,
            max_samples,
            // A byte bound is conservative for Groq's 224-token prompt window,
            // including Chinese and other terms that may use multiple tokens.
            max_prompt_bytes: (preset == Some(CloudAsrPreset::Groq)).then_some(224),
        }
    }

    fn validate_audio(self, samples: usize) -> Result<(), SpeechRecognitionError> {
        if samples > self.max_samples {
            return Err(SpeechRecognitionError::Protocol(
                "recording exceeds the transcription service upload limit".to_owned(),
            ));
        }
        Ok(())
    }
}

enum VocabularyHints {
    Prompt(Option<String>),
    ContextBias(Vec<String>),
}

/// Bounds dictionary context before mapping it to the provider's multipart fields.
fn vocabulary_hints(
    hints: &SpeechRecognitionHints,
    limits: TranscriptionLimits,
) -> VocabularyHints {
    let mut seen = std::collections::BTreeSet::new();
    let mut terms = Vec::new();
    let mut characters = 0;
    let mut bytes = 0;
    for term in hints.terms().iter().map(|term| term.trim()) {
        if term.is_empty()
            || term.chars().any(char::is_control)
            || !seen.insert(term.to_lowercase())
        {
            continue;
        }
        let separator = if terms.is_empty() { 0 } else { 2 };
        let term_characters = term.chars().count();
        if characters + separator + term_characters > MAX_PROMPT_CHARS
            || limits
                .max_prompt_bytes
                .is_some_and(|maximum| bytes + separator + term.len() > maximum)
        {
            continue;
        }
        terms.push(term.to_owned());
        characters += separator + term_characters;
        bytes += separator + term.len();
        if terms.len() == MAX_PROMPT_TERMS {
            break;
        }
    }
    if limits.preset == Some(CloudAsrPreset::Mistral) {
        VocabularyHints::ContextBias(terms)
    } else {
        VocabularyHints::Prompt((!terms.is_empty()).then(|| terms.join(", ")))
    }
}

#[derive(Deserialize)]
struct TranscriptionResponse {
    text: String,
}

fn transcription_endpoint(base_url: &str) -> Result<Url, SpeechRecognitionError> {
    let base_url = base_url.trim().trim_end_matches('/');
    if base_url.is_empty() {
        return Err(SpeechRecognitionError::NotConfigured);
    }
    let endpoint = if base_url.ends_with("/audio/transcriptions") {
        base_url.to_owned()
    } else {
        format!("{base_url}/audio/transcriptions")
    };
    let url = Url::parse(&endpoint).map_err(protocol_error)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(SpeechRecognitionError::Protocol(
            "transcription service address is invalid".to_owned(),
        ));
    }
    Ok(url)
}

fn wav_bytes(samples: &[i16]) -> Result<Vec<u8>, SpeechRecognitionError> {
    let data_size = samples
        .len()
        .checked_mul(2)
        .and_then(|size| u32::try_from(size).ok())
        .ok_or_else(|| SpeechRecognitionError::Protocol("recording is too large".to_owned()))?;
    let mut wav = Vec::with_capacity(44 + data_size as usize);
    wav.write_all(b"RIFF").map_err(protocol_error)?;
    wav.write_all(&(36_u32 + data_size).to_le_bytes())
        .map_err(protocol_error)?;
    wav.write_all(b"WAVEfmt ").map_err(protocol_error)?;
    wav.write_all(&16_u32.to_le_bytes())
        .map_err(protocol_error)?;
    wav.write_all(&1_u16.to_le_bytes())
        .map_err(protocol_error)?;
    wav.write_all(&1_u16.to_le_bytes())
        .map_err(protocol_error)?;
    wav.write_all(&SAMPLE_RATE.to_le_bytes())
        .map_err(protocol_error)?;
    wav.write_all(&(SAMPLE_RATE * 2).to_le_bytes())
        .map_err(protocol_error)?;
    wav.write_all(&2_u16.to_le_bytes())
        .map_err(protocol_error)?;
    wav.write_all(&16_u16.to_le_bytes())
        .map_err(protocol_error)?;
    wav.write_all(b"data").map_err(protocol_error)?;
    wav.write_all(&data_size.to_le_bytes())
        .map_err(protocol_error)?;
    for sample in samples {
        wav.write_all(&sample.to_le_bytes())
            .map_err(protocol_error)?;
    }
    Ok(wav)
}

fn http_error(status: StatusCode) -> SpeechRecognitionError {
    match status.as_u16() {
        401 | 403 => SpeechRecognitionError::Authentication,
        429 => SpeechRecognitionError::Quota,
        _ => SpeechRecognitionError::Protocol(format!(
            "transcription endpoint returned HTTP {}",
            status.as_u16()
        )),
    }
}

fn protocol_error(error: impl std::fmt::Display) -> SpeechRecognitionError {
    SpeechRecognitionError::Protocol(error.to_string())
}

fn transport_error(error: impl std::fmt::Display) -> SpeechRecognitionError {
    SpeechRecognitionError::Transport(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_openai_transcriptions_endpoint() {
        let endpoint = transcription_endpoint("https://api.example.com/v1");
        assert_eq!(
            Ok("https://api.example.com/v1/audio/transcriptions"),
            endpoint.as_ref().map(Url::as_str)
        );
    }

    #[test]
    #[allow(clippy::panic_in_result_fn)]
    fn sends_provider_specific_vocabulary_fields() -> Result<(), Box<dyn std::error::Error>> {
        use httpmock::{Method::POST, MockServer};

        for preset in CloudAsrPreset::ALL {
            let server = MockServer::start();
            let transcription = server.mock(|when, then| {
                let when = when
                    .method(POST)
                    .path("/audio/transcriptions")
                    .body_includes(preset.default_model())
                    .body_includes("filename=\"saymore.wav\"");
                if preset == CloudAsrPreset::Mistral {
                    when.body_includes("name=\"context_bias\"\r\n\r\nSaymore")
                        .body_includes("name=\"context_bias\"\r\n\r\nVoxtral")
                        .body_excludes("name=\"prompt\"");
                } else {
                    when.body_includes("name=\"prompt\"\r\n\r\nSaymore, Voxtral")
                        .body_excludes("name=\"context_bias\"");
                }
                then.status(200)
                    .json_body(serde_json::json!({"text": "Saymore Voxtral"}));
            });
            let mut recognizer =
                OpenAiCompatibleSpeechRecognizer::new(OpenAiCompatibleAsrSettings {
                    enabled: true,
                    base_url: preset.base_url().to_owned(),
                    api_key: "test-key".to_owned(),
                    model: preset.default_model().to_owned(),
                })?;
            // Replace only the transport destination; retain the real service's policy.
            recognizer.endpoint = Url::parse(&server.url("/audio/transcriptions"))?;
            let hints = SpeechRecognitionHints::from_terms(vec![
                "Saymore".to_owned(),
                "saymore".to_owned(),
                "Voxtral".to_owned(),
            ]);
            let session = recognizer.start(hints, Arc::new(|_| {}))?;
            session.push_audio(vec![1; 160])?;
            assert_eq!("Saymore Voxtral", session.finish()?);
            transcription.assert();
        }
        Ok(())
    }

    #[test]
    fn groq_bounds_utf8_prompts_and_keeps_later_short_terms() {
        let hints = SpeechRecognitionHints::from_terms(vec![
            "词".repeat(100),
            "声".repeat(73),
            "Slint".to_owned(),
            "UI".to_owned(),
        ]);
        let limits = TranscriptionLimits::for_endpoint(CloudAsrPreset::Groq.base_url());
        let VocabularyHints::Prompt(Some(prompt)) = vocabulary_hints(&hints, limits) else {
            panic!("Groq must receive a vocabulary prompt");
        };
        assert!(prompt.len() <= 224);
        assert_eq!(format!("{}, UI", "声".repeat(73)), prompt);
    }

    #[test]
    fn upload_limits_account_for_wav_headers_and_preserve_custom_limits() {
        for preset in [CloudAsrPreset::OpenAi, CloudAsrPreset::Groq] {
            let limits = TranscriptionLimits::for_endpoint(preset.base_url());
            let largest = (25_000_000 - 44) / 2;
            assert!(limits.validate_audio(largest).is_ok());
            assert!(limits.validate_audio(largest + 1).is_err());
        }
        let custom = TranscriptionLimits::for_endpoint("https://example.com/v1");
        assert!(custom.validate_audio(16_000 * 60 * 30).is_ok());
        assert!(custom.validate_audio(16_000 * 60 * 30 + 1).is_err());
    }

    #[test]
    fn encodes_mono_pcm_as_wav() {
        let wav = wav_bytes(&[1, -2]).unwrap_or_default();
        assert_eq!(b"RIFF", &wav[0..4]);
        assert_eq!(b"WAVE", &wav[8..12]);
        assert_eq!(48, wav.len());
        assert_eq!([1, 0, 254, 255], wav[44..48]);
    }
}
