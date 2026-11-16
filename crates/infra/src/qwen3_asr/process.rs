use serde_json::Value;
use std::{
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread::{self, JoinHandle},
    time::Duration,
};
use template_app::SpeechRecognitionError;

pub(super) fn protocol(message: &str) -> SpeechRecognitionError {
    SpeechRecognitionError::Protocol(format!("Qwen3 native worker {message}"))
}

pub(super) struct Process {
    pub child: Child,
    pub input: ChildStdin,
    pub responses: Receiver<Result<Value, SpeechRecognitionError>>,
    reader: Option<JoinHandle<()>>,
}

impl Process {
    pub fn spawn(executable: &Path, model: &Path) -> Result<Self, SpeechRecognitionError> {
        let mut command = Command::new(executable);
        command
            .arg("worker")
            .arg(model)
            .args(["--threads", "4", "--no-warmup", "--device"])
            .arg(if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
                "gpu"
            } else {
                "cpu"
            })
            .env("SPEECH_NO_UPDATE_CHECK", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let mut child = command
            .spawn()
            .map_err(|_| protocol("could not start; reinstall the application"))?;
        let input = child.stdin.take();
        let output = child.stdout.take();
        let (Some(input), Some(output)) = (input, output) else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(protocol("has no private pipes"));
        };
        let (send, responses) = mpsc::channel();
        let mut process = Self {
            child,
            input,
            responses,
            reader: None,
        };
        process.reader = Some(
            thread::Builder::new()
                .name("saymore-qwen-output".into())
                .spawn(move || {
                    let mut reader = BufReader::new(output);
                    loop {
                        let result = read_message(&mut reader);
                        let failed = result.is_err();
                        if send.send(result).is_err() || failed {
                            break;
                        }
                    }
                })
                .map_err(|_| protocol("could not start its response reader"))?,
        );
        Ok(process)
    }

    pub fn receive(&self, timeout: Duration) -> Result<Value, SpeechRecognitionError> {
        self.responses
            .recv_timeout(timeout)
            .map_err(|_| protocol("stopped responding"))?
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

fn read_message(reader: &mut impl BufRead) -> Result<Value, SpeechRecognitionError> {
    const MAX_MESSAGE_BYTES: u64 = 1024 * 1024;
    let mut bytes = Vec::new();
    let read = reader
        .take(MAX_MESSAGE_BYTES + 1)
        .read_until(b'\n', &mut bytes)
        .map_err(|_| protocol("response could not be read"))?;
    if read == 0 || read as u64 > MAX_MESSAGE_BYTES || bytes.last() != Some(&b'\n') {
        return Err(protocol("returned an incomplete or oversized response"));
    }
    serde_json::from_slice(&bytes).map_err(|_| protocol("returned invalid JSON"))
}

pub(super) fn executable() -> Result<PathBuf, SpeechRecognitionError> {
    let current =
        std::env::current_exe().map_err(|_| protocol("application location is unavailable"))?;
    let directory = current
        .parent()
        .ok_or_else(|| protocol("application has no directory"))?;
    let name = if cfg!(target_os = "windows") {
        "saymore-speech.exe"
    } else {
        "saymore-speech"
    };
    let adjacent = directory.join(name);
    if adjacent.is_file() {
        return Ok(adjacent);
    }
    if matches!(
        directory.file_name().and_then(|name| name.to_str()),
        Some("deps" | "examples")
    ) && let Some(parent) = directory.parent()
        && parent.join(name).is_file()
    {
        return Ok(parent.join(name));
    }
    Err(protocol("is missing; reinstall the application"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_corrupt_or_incomplete_protocol_without_exposing_its_contents() {
        for bytes in [
            b"private transcript\n".as_slice(),
            b"{\"type\":\"end\"}",
            b"",
        ] {
            let error = read_message(&mut std::io::Cursor::new(bytes)).err();
            assert!(error.is_some());
            assert!(!format!("{error:?}").contains("private transcript"));
        }
    }
}
