use super::{
    Qwen3Decoder, StreamingText,
    process::{Process, executable, protocol},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{
    io::Write,
    path::Path,
    sync::Mutex,
    time::{Duration, Instant},
};
use template_app::SpeechRecognitionError;

#[cfg(all(test, unix))]
#[path = "worker_tests.rs"]
mod worker_tests;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

enum Request<'a> {
    Segment,
    Streaming { prefix: &'a str },
}

pub(super) struct NativeDecoder {
    process: Mutex<Option<Process>>,
}

impl NativeDecoder {
    pub fn load(model: &Path) -> Result<Self, SpeechRecognitionError> {
        let process = Process::spawn(&executable()?, model)?;
        let ready = process.receive(REQUEST_TIMEOUT)?;
        validate_ready(&ready)?;
        tracing::info!(target: "saymore::diagnostics", event = "asr.qwen.loaded",
            backend = if cfg!(all(target_os = "macos", target_arch = "aarch64")) { "metal" } else { "cpu" });
        Ok(Self {
            process: Mutex::new(Some(process)),
        })
    }
    fn request(
        &self,
        samples: &[i16],
        hotwords: &str,
        request: Request<'_>,
    ) -> Result<StreamingText, SpeechRecognitionError> {
        let mut guard = self
            .process
            .lock()
            .map_err(|_| protocol("lock was poisoned"))?;
        let process = guard
            .as_mut()
            .ok_or_else(|| protocol("is unavailable; reload the model"))?;
        let result = transcribe(process, samples, hotwords, request);
        if result.is_err() {
            guard.take();
        }
        result
    }
}

impl Qwen3Decoder for NativeDecoder {
    fn decode(&self, samples: &[i16], hotwords: &str) -> Result<String, SpeechRecognitionError> {
        self.request(samples, hotwords, Request::Segment)
            .map(|result| result.text)
    }

    fn decode_stream(
        &self,
        samples: &[i16],
        hotwords: &str,
        prefix: &str,
    ) -> Result<StreamingText, SpeechRecognitionError> {
        self.request(samples, hotwords, Request::Streaming { prefix })
    }

    fn resident_memory_bytes(&self) -> Option<u64> {
        let guard = self.process.try_lock().ok()?;
        crate::process_memory::process_resident_memory_bytes(guard.as_ref()?.child.id())
    }
}

fn transcribe(
    process: &mut Process,
    samples: &[i16],
    hotwords: &str,
    request: Request<'_>,
) -> Result<StreamingText, SpeechRecognitionError> {
    let deadline = Instant::now() + REQUEST_TIMEOUT;
    for (seq, chunk) in samples.chunks(16_000).enumerate() {
        let bytes = chunk
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect::<Vec<_>>();
        send(
            &mut process.input,
            &json!({"type": "chunk", "id": "decode", "seq": seq, "pcm": STANDARD.encode(bytes)}),
        )?;
    }
    // Explicit empty prompt clears vocabulary between requests in the resident worker.
    let mut options =
        json!({"type": "transcribe", "id": "decode", "sample_rate": 16000, "prompt": hotwords});
    let streaming = match request {
        Request::Segment => false,
        Request::Streaming { prefix } => {
            options["prefix"] = json!(prefix);
            options["rollback_tokens"] = json!(5);
            options["max_new_tokens"] = json!(128);
            true
        }
    };
    send(&mut process.input, &options)?;
    process
        .input
        .flush()
        .map_err(|_| protocol("audio could not be submitted"))?;
    loop {
        let response = process.receive(deadline.saturating_duration_since(Instant::now()))?;
        if response.get("id").and_then(Value::as_str) != Some("decode") {
            return Err(protocol("returned a different request identity"));
        }
        match response.get("type").and_then(Value::as_str) {
            Some("progress") => continue,
            Some("end") => {
                let text = completed_text(&response)?;
                let raw = if streaming {
                    response["raw_text"]
                        .as_str()
                        .ok_or_else(|| protocol("returned no continuation state"))?
                        .to_owned()
                } else {
                    String::new()
                };
                return Ok(StreamingText { text, raw });
            }
            _ => return Err(protocol("recognition failed")),
        }
    }
}

fn send(output: &mut impl Write, message: &Value) -> Result<(), SpeechRecognitionError> {
    serde_json::to_writer(&mut *output, message)
        .map_err(|_| protocol("request could not be written"))?;
    output
        .write_all(b"\n")
        .map_err(|_| protocol("request could not be written"))
}

fn validate_ready(ready: &Value) -> Result<(), SpeechRecognitionError> {
    let model = &ready["model"];
    let prompt = model["options"].as_array().is_some_and(|options| {
        options.iter().any(|option| {
            option["name"] == "prompt" && option["type"] == "string" && option["steers"] == true
        })
    });
    let continuation = model["options"].as_array().is_some_and(|options| {
        ["prefix", "rollback_tokens", "max_new_tokens"]
            .iter()
            .all(|name| options.iter().any(|option| option["name"] == *name))
    });
    let device = model["device"].as_str().unwrap_or_default();
    let correct_device = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        device.starts_with("MTL")
    } else {
        device == "CPU"
    };
    if ready["type"] != "ready"
        || ready["protocol"] != 3
        || ready["version"] != "0.8.11"
        || model["name"] != "Qwen3-ASR-1.7B"
        || model["weight_type"] != "Q8_0"
        || model["sample_rate"] != 16000
        || !prompt
        || !continuation
        || !correct_device
    {
        return Err(protocol(
            "does not match the pinned model, runtime or compute backend",
        ));
    }
    Ok(())
}

fn completed_text(response: &Value) -> Result<String, SpeechRecognitionError> {
    if response["stop"] != "complete" {
        return Err(protocol(
            "reached its model limit; no truncated transcript was accepted",
        ));
    }
    response["text"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| protocol("returned no transcript field"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_truncated_success_and_missing_text() {
        assert!(completed_text(&json!({"stop":"model_limit","text":"partial"})).is_err());
        assert!(completed_text(&json!({"stop":"complete"})).is_err());
        assert_eq!(
            Ok("".to_owned()),
            completed_text(&json!({"stop":"complete","text":""}))
        );
        assert_eq!(
            Ok("hello".to_owned()),
            completed_text(&json!({"stop":"complete","text":"hello"}))
        );
    }
}
