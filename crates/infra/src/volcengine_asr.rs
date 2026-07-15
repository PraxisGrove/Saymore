use std::{
    io::{Read, Write},
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};

use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use template_app::{
    SpeechRecognitionError, StreamingRecognitionSession, StreamingSpeechRecognizer,
};
use tokio::sync::mpsc as tokio_mpsc;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};
use uuid::Uuid;

const ENDPOINT: &str = "wss://openspeech.bytedance.com/api/v3/sauc/bigmodel_async";
const RESOURCE_ID: &str = "volc.seedasr.sauc.duration";
const FINAL_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECTION_TEST_TIMEOUT: Duration = Duration::from_secs(8);
const AUDIO_QUEUE_CAPACITY: usize = 128;

pub struct VolcengineSpeechRecognizer {
    api_key: String,
}

impl VolcengineSpeechRecognizer {
    pub fn new(api_key: String) -> Result<Self, SpeechRecognitionError> {
        let api_key = api_key.trim();
        if api_key.is_empty() {
            return Err(SpeechRecognitionError::NotConfigured);
        }
        Ok(Self {
            api_key: api_key.to_owned(),
        })
    }

    pub async fn test_connection(&self) -> Result<(), SpeechRecognitionError> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let request = connection_request(&self.api_key)?;
        let (mut socket, _) = connect_async(request).await.map_err(handshake_error)?;
        socket
            .send(Message::Binary(encode_config_request()?.into()))
            .await
            .map_err(transport_error)?;
        socket
            .send(Message::Binary(encode_audio_request(-2, &[], true)?.into()))
            .await
            .map_err(transport_error)?;
        let response = tokio::time::timeout(CONNECTION_TEST_TIMEOUT, socket.next())
            .await
            .map_err(|_| SpeechRecognitionError::Timeout)?;
        let result = match response {
            Some(Ok(Message::Binary(bytes))) => parse_server_message(&bytes).map(|_| ()),
            Some(Ok(Message::Close(_))) | None => Err(SpeechRecognitionError::Transport(
                "ASR connection closed during testing".to_owned(),
            )),
            Some(Ok(_)) => Err(SpeechRecognitionError::Protocol(
                "ASR test response is not binary".to_owned(),
            )),
            Some(Err(error)) => Err(transport_error(error)),
        };
        let _ = socket.close(None).await;
        result
    }
}

impl StreamingSpeechRecognizer for VolcengineSpeechRecognizer {
    fn start(
        &self,
        on_partial: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Result<Box<dyn StreamingRecognitionSession>, SpeechRecognitionError> {
        let (command_tx, command_rx) = tokio_mpsc::channel(AUDIO_QUEUE_CAPACITY);
        let (result_tx, result_rx) = mpsc::sync_channel(1);
        let api_key = self.api_key.clone();
        thread::Builder::new()
            .name("saymore-volcengine-asr".to_owned())
            .spawn(move || {
                let result = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|error| SpeechRecognitionError::Transport(error.to_string()))
                    .and_then(|runtime| {
                        runtime.block_on(run_session(api_key, command_rx, on_partial))
                    });
                let _ = result_tx.send(result);
            })
            .map_err(|error| SpeechRecognitionError::Transport(error.to_string()))?;

        Ok(Box::new(VolcengineSession {
            command_tx,
            result_rx,
        }))
    }
}

struct VolcengineSession {
    command_tx: tokio_mpsc::Sender<SessionCommand>,
    result_rx: mpsc::Receiver<Result<String, SpeechRecognitionError>>,
}

impl StreamingRecognitionSession for VolcengineSession {
    fn push_audio(&self, samples: Vec<i16>) -> Result<(), SpeechRecognitionError> {
        self.command_tx
            .try_send(SessionCommand::Audio(samples))
            .map_err(|error| match error {
                tokio_mpsc::error::TrySendError::Full(_) => {
                    SpeechRecognitionError::Transport("ASR audio queue is full".to_owned())
                }
                tokio_mpsc::error::TrySendError::Closed(_) => {
                    SpeechRecognitionError::Transport("ASR session stopped".to_owned())
                }
            })
    }

    fn finish(self: Box<Self>) -> Result<String, SpeechRecognitionError> {
        let _ = self.command_tx.blocking_send(SessionCommand::Finish);
        self.result_rx
            .recv_timeout(FINAL_TIMEOUT + Duration::from_secs(2))
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => SpeechRecognitionError::Timeout,
                mpsc::RecvTimeoutError::Disconnected => {
                    SpeechRecognitionError::Transport("ASR worker stopped".to_owned())
                }
            })?
    }

    fn cancel(self: Box<Self>) {
        let _ = self.command_tx.try_send(SessionCommand::Cancel);
    }
}

enum SessionCommand {
    Audio(Vec<i16>),
    Finish,
    Cancel,
}

async fn run_session(
    api_key: String,
    mut commands: tokio_mpsc::Receiver<SessionCommand>,
    on_partial: Arc<dyn Fn(String) + Send + Sync>,
) -> Result<String, SpeechRecognitionError> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let request = connection_request(&api_key)?;

    let (mut socket, _) = connect_async(request).await.map_err(handshake_error)?;
    socket
        .send(Message::Binary(encode_config_request()?.into()))
        .await
        .map_err(transport_error)?;

    let mut sequence = 2_i32;
    let mut finishing = false;
    let timeout = tokio::time::sleep(FINAL_TIMEOUT);
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            command = commands.recv(), if !finishing => {
                match command {
                    Some(SessionCommand::Audio(samples)) => {
                        if !samples.is_empty() {
                            socket.send(Message::Binary(encode_audio_request(sequence, &samples, false)?.into()))
                                .await
                                .map_err(transport_error)?;
                            sequence = sequence.saturating_add(1);
                        }
                    }
                    Some(SessionCommand::Finish) => {
                        socket.send(Message::Binary(encode_audio_request(-sequence, &[], true)?.into()))
                            .await
                            .map_err(transport_error)?;
                        finishing = true;
                        timeout.as_mut().reset(tokio::time::Instant::now() + FINAL_TIMEOUT);
                    }
                    Some(SessionCommand::Cancel) | None => {
                        let _ = socket.close(None).await;
                        return Err(SpeechRecognitionError::Transport("ASR session cancelled".to_owned()));
                    }
                }
            }
            message = socket.next() => {
                match message {
                    Some(Ok(Message::Binary(bytes))) => {
                        if let Some(transcript) = parse_server_message(&bytes)? {
                            let text = transcript.text;
                            on_partial(text.clone());
                            if finishing && transcript.is_final {
                                let _ = socket.close(None).await;
                                return Ok(text);
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        return Err(SpeechRecognitionError::Transport("ASR connection closed before a final result".to_owned()));
                    }
                    Some(Ok(_)) => {}
                    Some(Err(error)) => return Err(transport_error(error)),
                }
            }
            () = &mut timeout, if finishing => return Err(SpeechRecognitionError::Timeout),
        }
    }
}

fn connection_request(
    api_key: &str,
) -> Result<tokio_tungstenite::tungstenite::http::Request<()>, SpeechRecognitionError> {
    let mut request = ENDPOINT.into_client_request().map_err(protocol_error)?;
    let headers = request.headers_mut();
    headers.insert("X-Api-Key", header_value(api_key)?);
    headers.insert("X-Api-Resource-Id", header_value(RESOURCE_ID)?);
    headers.insert(
        "X-Api-Connect-Id",
        header_value(&Uuid::new_v4().to_string())?,
    );
    Ok(request)
}

fn encode_config_request() -> Result<Vec<u8>, SpeechRecognitionError> {
    let payload = json!({
        "user": { "uid": Uuid::new_v4().to_string() },
        "audio": {
            "format": "pcm",
            "codec": "raw",
            "rate": 16000,
            "bits": 16,
            "channel": 1
        },
        "request": {
            "model_name": "bigmodel",
            "enable_itn": true,
            "enable_punc": true,
            "enable_ddc": true,
            "show_utterances": true,
            "enable_nonstream": false,
            "result_type": "full"
        }
    });
    encode_packet(
        0x1,
        0,
        0,
        &serde_json::to_vec(&payload).map_err(protocol_error)?,
    )
}

fn encode_audio_request(
    sequence: i32,
    samples: &[i16],
    final_packet: bool,
) -> Result<Vec<u8>, SpeechRecognitionError> {
    let mut pcm = Vec::with_capacity(samples.len() * 2);
    for sample in samples {
        pcm.extend_from_slice(&sample.to_le_bytes());
    }
    encode_packet(0x2, if final_packet { 0x3 } else { 0x1 }, sequence, &pcm)
}

fn encode_packet(
    message_type: u8,
    flags: u8,
    sequence: i32,
    payload: &[u8],
) -> Result<Vec<u8>, SpeechRecognitionError> {
    let compressed = gzip(payload)?;
    let mut packet = Vec::with_capacity(12 + compressed.len());
    packet.extend_from_slice(&[0x11, (message_type << 4) | flags, 0x11, 0x00]);
    if flags & 0x1 != 0 {
        packet.extend_from_slice(&sequence.to_be_bytes());
    }
    packet.extend_from_slice(&payload_size(compressed.len())?);
    packet.extend_from_slice(&compressed);
    Ok(packet)
}

#[derive(Debug, PartialEq, Eq)]
struct ProviderTranscript {
    text: String,
    is_final: bool,
}

fn parse_server_message(
    bytes: &[u8],
) -> Result<Option<ProviderTranscript>, SpeechRecognitionError> {
    if bytes.len() < 4 {
        return Err(SpeechRecognitionError::Protocol(
            "response header is truncated".to_owned(),
        ));
    }
    let header_len = usize::from(bytes[0] & 0x0f) * 4;
    if header_len < 4 || bytes.len() < header_len {
        return Err(SpeechRecognitionError::Protocol(
            "response header length is invalid".to_owned(),
        ));
    }
    let message_type = bytes[1] >> 4;
    let flags = bytes[1] & 0x0f;
    let compression = bytes[2] & 0x0f;
    let mut offset = header_len;
    if flags & 0x1 != 0 {
        take_u32(bytes, &mut offset)?;
    }

    if message_type == 0x0f {
        let code = take_u32(bytes, &mut offset)?;
        let message = read_payload(bytes, &mut offset, compression)?;
        return Err(provider_error(code, &String::from_utf8_lossy(&message)));
    }
    if message_type != 0x09 {
        return Ok(None);
    }
    let payload = read_payload(bytes, &mut offset, compression)?;
    let value: Value = serde_json::from_slice(&payload).map_err(protocol_error)?;
    Ok(transcript_text(&value).map(|text| ProviderTranscript {
        text,
        is_final: flags & 0x2 != 0,
    }))
}

fn read_payload(
    bytes: &[u8],
    offset: &mut usize,
    compression: u8,
) -> Result<Vec<u8>, SpeechRecognitionError> {
    let size = usize::try_from(take_u32(bytes, offset)?).map_err(protocol_error)?;
    let end = offset.saturating_add(size);
    let payload = bytes.get(*offset..end).ok_or_else(|| {
        SpeechRecognitionError::Protocol("response payload is truncated".to_owned())
    })?;
    *offset = end;
    match compression {
        0 => Ok(payload.to_vec()),
        1 => gunzip(payload),
        _ => Err(SpeechRecognitionError::Protocol(
            "response compression is unsupported".to_owned(),
        )),
    }
}

fn transcript_text(value: &Value) -> Option<String> {
    value
        .pointer("/result/text")
        .or_else(|| value.pointer("/payload_msg/result/text"))
        .or_else(|| value.get("text"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
}

fn take_u32(bytes: &[u8], offset: &mut usize) -> Result<u32, SpeechRecognitionError> {
    let end = offset.saturating_add(4);
    let value = bytes
        .get(*offset..end)
        .and_then(|slice| <[u8; 4]>::try_from(slice).ok())
        .ok_or_else(|| {
            SpeechRecognitionError::Protocol("response field is truncated".to_owned())
        })?;
    *offset = end;
    Ok(u32::from_be_bytes(value))
}

fn gzip(payload: &[u8]) -> Result<Vec<u8>, SpeechRecognitionError> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(payload).map_err(protocol_error)?;
    encoder.finish().map_err(protocol_error)
}

fn gunzip(payload: &[u8]) -> Result<Vec<u8>, SpeechRecognitionError> {
    let mut decoder = GzDecoder::new(payload);
    let mut decoded = Vec::new();
    decoder.read_to_end(&mut decoded).map_err(protocol_error)?;
    Ok(decoded)
}

fn payload_size(size: usize) -> Result<[u8; 4], SpeechRecognitionError> {
    u32::try_from(size)
        .map(u32::to_be_bytes)
        .map_err(protocol_error)
}

fn header_value(
    value: &str,
) -> Result<tokio_tungstenite::tungstenite::http::HeaderValue, SpeechRecognitionError> {
    value.parse().map_err(protocol_error)
}

fn handshake_error(error: tokio_tungstenite::tungstenite::Error) -> SpeechRecognitionError {
    if let tokio_tungstenite::tungstenite::Error::Http(response) = &error {
        tracing::warn!(
            target: "saymore::diagnostics",
            event = "asr.handshake_rejected",
            status = response.status().as_u16()
        );
        return http_status_error(response.status().as_u16());
    }
    transport_error(error)
}

fn http_status_error(status: u16) -> SpeechRecognitionError {
    match status {
        401 | 403 => SpeechRecognitionError::Authentication,
        429 => SpeechRecognitionError::Quota,
        _ => SpeechRecognitionError::Transport(format!("ASR endpoint returned HTTP {status}")),
    }
}

fn provider_error(code: u32, message: &str) -> SpeechRecognitionError {
    tracing::warn!(
        target: "saymore::diagnostics",
        event = "asr.provider_rejected",
        code,
        reason = message
    );
    match code {
        401 | 403 => SpeechRecognitionError::Authentication,
        429 | 55000000..=55999999 => SpeechRecognitionError::Quota,
        _ => SpeechRecognitionError::Protocol(format!("provider error {code}: {message}")),
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
    use std::{env, fs};

    use super::*;

    #[test]
    fn connection_request_contains_volcengine_credentials_and_resource() {
        let Ok(request) = connection_request("test-key") else {
            panic!("connection request should be valid");
        };

        assert_eq!(
            Some("test-key"),
            request
                .headers()
                .get("X-Api-Key")
                .and_then(|value| value.to_str().ok())
        );
        assert_eq!(
            Some(RESOURCE_ID),
            request
                .headers()
                .get("X-Api-Resource-Id")
                .and_then(|value| value.to_str().ok())
        );
        assert!(request.headers().contains_key("X-Api-Connect-Id"));
    }

    #[test]
    fn encodes_pcm_audio_as_sequenced_gzip_packet() {
        let Ok(packet) = encode_audio_request(7, &[1, -2], false) else {
            panic!("audio packet should encode");
        };
        assert_eq!([0x11, 0x21, 0x11, 0x00], packet[..4]);
        assert_eq!(7_i32.to_be_bytes(), packet[4..8]);
        let size = u32::from_be_bytes([packet[8], packet[9], packet[10], packet[11]]) as usize;
        let Ok(decoded) = gunzip(&packet[12..12 + size]) else {
            panic!("audio packet should decompress");
        };
        assert_eq!([1, 0, 254, 255], decoded.as_slice());
    }

    #[test]
    fn parses_gzip_transcript_response() {
        let payload = gzip(br#"{"result":{"text":"  Saymore test  "}}"#).unwrap_or_default();
        let mut response = vec![0x11, 0x93, 0x11, 0x00];
        response.extend_from_slice(&(-3_i32).to_be_bytes());
        response.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        response.extend_from_slice(&payload);

        assert_eq!(
            Ok(Some(ProviderTranscript {
                text: "Saymore test".to_owned(),
                is_final: true,
            })),
            parse_server_message(&response)
        );
    }

    #[test]
    fn maps_provider_authentication_errors() {
        let message = gzip(b"invalid credential").unwrap_or_default();
        let mut response = vec![0x11, 0xf0, 0x11, 0x00];
        response.extend_from_slice(&403_u32.to_be_bytes());
        response.extend_from_slice(&(message.len() as u32).to_be_bytes());
        response.extend_from_slice(&message);

        assert_eq!(
            Err(SpeechRecognitionError::Authentication),
            parse_server_message(&response)
        );
    }

    #[test]
    #[ignore = "requires a live Volcengine key and synthetic WAV fixture"]
    fn transcribes_a_live_wav_fixture() {
        let Ok(api_key) = env::var("SAYMORE_VOLCENGINE_API_KEY") else {
            panic!("SAYMORE_VOLCENGINE_API_KEY is required");
        };
        let Ok(path) = env::var("SAYMORE_ASR_WAV") else {
            panic!("SAYMORE_ASR_WAV is required");
        };
        let Ok(wav) = fs::read(path) else {
            panic!("WAV fixture should be readable");
        };
        let Ok(samples) = pcm_samples_from_wav(&wav) else {
            panic!("WAV fixture should contain mono PCM16 audio");
        };
        let Ok(recognizer) = VolcengineSpeechRecognizer::new(api_key) else {
            panic!("live recognizer should be configured");
        };
        let Ok(session) = recognizer.start(Arc::new(|_| {})) else {
            panic!("live session should start");
        };
        for chunk in samples.chunks(1_600) {
            assert!(session.push_audio(chunk.to_vec()).is_ok());
        }
        let transcript = match session.finish() {
            Ok(transcript) => transcript,
            Err(error) => panic!("live transcription should succeed: {error}"),
        };
        assert!(!transcript.trim().is_empty());
    }

    fn pcm_samples_from_wav(wav: &[u8]) -> Result<Vec<i16>, ()> {
        if wav.get(0..4) != Some(b"RIFF") || wav.get(8..12) != Some(b"WAVE") {
            return Err(());
        }
        let mut offset = 12_usize;
        while offset.saturating_add(8) <= wav.len() {
            let id = wav.get(offset..offset + 4).ok_or(())?;
            let size = wav
                .get(offset + 4..offset + 8)
                .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
                .map(u32::from_le_bytes)
                .and_then(|size| usize::try_from(size).ok())
                .ok_or(())?;
            let data_start = offset + 8;
            let data_end = data_start.saturating_add(size);
            if id == b"data" {
                return wav
                    .get(data_start..data_end)
                    .ok_or(())?
                    .chunks_exact(2)
                    .map(|bytes| {
                        <[u8; 2]>::try_from(bytes)
                            .map(i16::from_le_bytes)
                            .map_err(|_| ())
                    })
                    .collect();
            }
            offset = data_end.saturating_add(size % 2);
        }
        Err(())
    }
}
