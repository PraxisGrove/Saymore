use std::{
    fs,
    io::{self, BufRead, BufReader, Write},
    os::unix::{fs::PermissionsExt, net::UnixListener},
    path::Path,
    thread::{self, JoinHandle},
};

use template_infra::text_control_capabilities_for_process;

pub const SOCKET_PATH: &str = "/tmp/com.saymore.desktop.preview.ax-probe.sock";

pub struct AxCompatibilityServer {
    _worker: JoinHandle<()>,
}

pub fn start() -> Result<AxCompatibilityServer, io::Error> {
    remove_stale_socket()?;
    let listener = UnixListener::bind(SOCKET_PATH)?;
    fs::set_permissions(SOCKET_PATH, fs::Permissions::from_mode(0o600))?;
    let worker = thread::Builder::new()
        .name("saymore-ax-compatibility".to_owned())
        .spawn(move || serve(listener))?;
    Ok(AxCompatibilityServer { _worker: worker })
}

fn remove_stale_socket() -> Result<(), io::Error> {
    match fs::remove_file(SOCKET_PATH) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn serve(listener: UnixListener) {
    for connection in listener.incoming() {
        match connection {
            Ok(mut stream) => {
                let response = read_process_id(&stream)
                    .and_then(probe_response)
                    .unwrap_or_else(|error| format!("ERROR: {error}"));
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.write_all(b"\n");
            }
            Err(error) => {
                tracing::warn!(event = "ax_compatibility.accept_failed", reason = %error);
            }
        }
    }
}

fn read_process_id(stream: &std::os::unix::net::UnixStream) -> Result<i32, io::Error> {
    let mut request = String::new();
    BufReader::new(stream).read_line(&mut request)?;
    request
        .trim()
        .parse::<i32>()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid process id"))
}

fn probe_response(process_id: i32) -> Result<String, io::Error> {
    let capabilities = text_control_capabilities_for_process(process_id)
        .map_err(|error| io::Error::other(error.to_string()))?;
    serde_json::to_string_pretty(&capabilities).map_err(io::Error::other)
}

impl Drop for AxCompatibilityServer {
    fn drop(&mut self) {
        if Path::new(SOCKET_PATH).exists() {
            let _ = fs::remove_file(SOCKET_PATH);
        }
    }
}
