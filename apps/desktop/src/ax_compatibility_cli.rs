use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    process::ExitCode,
};

use crate::ax_compatibility_server::SOCKET_PATH;

const COMMAND: &str = "--probe-focused-text-control";

pub fn run_if_requested() -> Option<ExitCode> {
    let requested = std::env::args().nth(1).as_deref() == Some(COMMAND);
    requested.then(run)
}

fn run() -> ExitCode {
    match requested_process_id().and_then(request_probe) {
        Ok(report) => {
            println!("{report}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("focused text control probe failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn requested_process_id() -> Result<i32, template_app::TextDeliveryError> {
    std::env::args()
        .nth(2)
        .ok_or_else(|| {
            template_app::TextDeliveryError::System(
                "focused text control probe requires a PID".to_owned(),
            )
        })?
        .parse::<i32>()
        .map_err(|_| {
            template_app::TextDeliveryError::System(
                "focused text control probe PID is invalid".to_owned(),
            )
        })
}

fn request_probe(process_id: i32) -> Result<String, template_app::TextDeliveryError> {
    let mut stream = UnixStream::connect(SOCKET_PATH).map_err(probe_error)?;
    writeln!(stream, "{process_id}").map_err(probe_error)?;
    let mut response = String::new();
    stream.read_to_string(&mut response).map_err(probe_error)?;
    if let Some(reason) = response.trim().strip_prefix("ERROR: ") {
        Err(template_app::TextDeliveryError::System(reason.to_owned()))
    } else {
        Ok(response.trim().to_owned())
    }
}

fn probe_error(error: std::io::Error) -> template_app::TextDeliveryError {
    template_app::TextDeliveryError::System(error.to_string())
}
