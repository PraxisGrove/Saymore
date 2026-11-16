#![allow(clippy::panic_in_result_fn)]

use super::*;
use std::{fs, os::unix::fs::PermissionsExt, sync::Mutex};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
#[ignore = "requires SAYMORE_QWEN_MODEL_DIR"]
fn native_prefix_is_forced_continuation_and_is_cleared_on_the_next_request() -> TestResult {
    let directory = std::path::PathBuf::from(
        std::env::var_os("SAYMORE_QWEN_MODEL_DIR").ok_or("model directory required")?,
    );
    let decoder = NativeDecoder::load(&directory.join("Qwen3-ASR-1.7B-Q8_0.gguf"))?;
    let audio = include_bytes!("../../../../apps/desktop/assets/asr-test/standard-zh.pcm")
        .as_chunks::<2>()
        .0
        .iter()
        .map(|bytes| i16::from_le_bytes(*bytes))
        .collect::<Vec<_>>();
    let result = decoder.decode_stream(
        &audio,
        "",
        "language Chinese<asr_text>这是前缀保留检查，前面的字必须保留。最后五个令牌用于回退。",
    )?;
    assert!(result.text.starts_with("这是前缀保留检查"));
    assert!(!result.raw.contains('\u{fffd}'));
    let fresh = decoder.decode_stream(&audio, "", "")?;
    assert_eq!("可以通过语音进行人机交互。", fresh.text);
    Ok(())
}

fn worker(reply: &str) -> Result<(tempfile::TempDir, NativeDecoder), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let executable = directory.path().join("fake-worker");
    fs::write(
        &executable,
        format!(
            r##"#!/bin/sh
while IFS= read -r line; do
    printf '%s\n' "$line" >> "$2/requests.jsonl"
    case "$line" in
        *'"type":"transcribe"'*) printf '%s\n' '{reply}' ;;
    esac
done
"##
        ),
    )?;
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))?;
    let process = Process::spawn(&executable, directory.path())?;
    Ok((
        directory,
        NativeDecoder {
            process: Mutex::new(Some(process)),
        },
    ))
}

#[test]
fn serial_requests_send_pcm_and_clear_previous_vocabulary() -> TestResult {
    let (directory, decoder) =
        worker(r#"{"type":"end","id":"decode","stop":"complete","text":"hello"}"#)?;
    assert_eq!("hello", decoder.decode(&[1, -2, 3], "Saymore 声墨")?);
    assert_eq!("hello", decoder.decode(&[4], "")?);
    let messages = fs::read_to_string(directory.path().join("requests.jsonl"))?
        .lines()
        .map(serde_json::from_str::<Value>)
        .collect::<Result<Vec<_>, _>>()?;
    let chunks = messages
        .iter()
        .filter(|message| message["type"] == "chunk")
        .collect::<Vec<_>>();
    assert_eq!(STANDARD.encode([1, 0, 254, 255, 3, 0]), chunks[0]["pcm"]);
    assert_eq!(STANDARD.encode([4, 0]), chunks[1]["pcm"]);
    let prompts = messages
        .iter()
        .filter(|message| message["type"] == "transcribe")
        .map(|message| message["prompt"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(vec![Some("Saymore 声墨"), Some("")], prompts);
    Ok(())
}

#[test]
fn invalid_terminal_responses_disable_the_failed_worker() -> TestResult {
    for reply in [
        r#"{"type":"end","id":"decode","stop":"model_limit","text":"partial"}"#,
        r#"{"type":"end","id":"other","stop":"complete","text":"stale"}"#,
        r#"{"type":"error","id":"decode","error":{"message":"private contents"}}"#,
        "broken JSON",
    ] {
        let (_directory, decoder) = worker(reply)?;
        let first = decoder.decode(&[1], "");
        assert!(first.is_err());
        assert!(!format!("{first:?}").contains("private contents"));
        assert!(decoder.decode(&[1], "").is_err());
    }
    Ok(())
}

#[test]
fn loading_requires_the_pinned_runtime_model_and_prompt_capability() {
    let device = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "MTL0"
    } else {
        "CPU"
    };
    let ready = json!({"type":"ready", "protocol":3, "version":"0.8.11", "model":{
        "name":"Qwen3-ASR-1.7B", "weight_type":"Q8_0", "sample_rate":16000,
        "device":device, "options":[{"name":"prompt","type":"string","steers":true}, {"name":"prefix"}, {"name":"rollback_tokens"}, {"name":"max_new_tokens"}]
    }});
    assert!(validate_ready(&ready).is_ok());
    for (field, value) in [
        ("weight_type", json!("F16")),
        ("device", json!("unknown")),
        ("options", json!([])),
    ] {
        let mut invalid = ready.clone();
        invalid["model"][field] = value;
        assert!(validate_ready(&invalid).is_err());
    }
}

#[test]
fn dropping_a_worker_reaps_its_child_and_a_silent_child_times_out() -> TestResult {
    let (_directory, decoder) =
        worker(r#"{"type":"end","id":"decode","stop":"complete","text":"hello"}"#)?;
    let mut guard = decoder.process.lock().map_err(|_| "lock poisoned")?;
    let process = guard.take().ok_or("missing process")?;
    assert!(process.receive(Duration::from_millis(20)).is_err());
    let pid = process.child.id();
    drop(process);
    let alive = std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()?
        .success();
    assert!(!alive);
    Ok(())
}
