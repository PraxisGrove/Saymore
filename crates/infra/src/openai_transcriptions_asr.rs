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
    OpenAiCompatibleAsrSettings, SpeechRecognitionError, SpeechRecognitionHints,
    StreamingRecognitionSession, StreamingSpeechRecognizer,
};

const SAMPLE_RATE: u32 = 16_000;
const MAX_SAMPLES: usize = SAMPLE_RATE as usize * 60 * 30;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(90);

pub struct OpenAiCompatibleSpeechRecognizer {
    client: Client,
    endpoint: Url,
    api_key: String,
    model: String,
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
        })
    }

    pub async fn test_connection(&self) -> Result<(), SpeechRecognitionError> {
        self.transcribe(vec![0; 1_600], true).await.map(|_| ())
    }

    async fn transcribe(
        &self,
        samples: Vec<i16>,
        allow_empty: bool,
    ) -> Result<String, SpeechRecognitionError> {
        let audio = Part::bytes(wav_bytes(&samples)?)
            .file_name("saymore.wav")
            .mime_str("audio/wav")
            .map_err(protocol_error)?;
        let response = self
            .client
            .post(self.endpoint.clone())
            .bearer_auth(&self.api_key)
            .multipart(
                Form::new()
                    .text("model", self.model.clone())
                    .part("file", audio),
            )
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
        _hints: SpeechRecognitionHints,
        _on_partial: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Result<Box<dyn StreamingRecognitionSession>, SpeechRecognitionError> {
        Ok(Box::new(OpenAiCompatibleSession {
            client: self.client.clone(),
            endpoint: self.endpoint.clone(),
            api_key: self.api_key.clone(),
            model: self.model.clone(),
            samples: Mutex::new(Vec::new()),
        }))
    }
}

struct OpenAiCompatibleSession {
    client: Client,
    endpoint: Url,
    api_key: String,
    model: String,
    samples: Mutex<Vec<i16>>,
}

impl StreamingRecognitionSession for OpenAiCompatibleSession {
    fn push_audio(&self, samples: Vec<i16>) -> Result<(), SpeechRecognitionError> {
        let mut buffered = self.samples.lock().map_err(|_| {
            SpeechRecognitionError::Transport("ASR audio lock was poisoned".to_owned())
        })?;
        if buffered.len().saturating_add(samples.len()) > MAX_SAMPLES {
            return Err(SpeechRecognitionError::Protocol(
                "recording exceeds the 30 minute compatibility limit".to_owned(),
            ));
        }
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
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(transport_error)?;
        runtime.block_on(recognizer.transcribe(samples, false))
    }

    fn cancel(self: Box<Self>) {}
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
    fn encodes_mono_pcm_as_wav() {
        let wav = wav_bytes(&[1, -2]).unwrap_or_default();
        assert_eq!(b"RIFF", &wav[0..4]);
        assert_eq!(b"WAVE", &wav[8..12]);
        assert_eq!(48, wav.len());
        assert_eq!([1, 0, 254, 255], wav[44..48]);
    }
}
