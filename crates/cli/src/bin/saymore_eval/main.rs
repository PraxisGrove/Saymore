#[cfg(target_os = "macos")]
use std::env;
use std::process::ExitCode;

#[cfg(target_os = "macos")]
use anyhow::Context;
use anyhow::{Result, bail};
#[cfg(target_os = "macos")]
use template_infra::AppEnvironment;

#[cfg(target_os = "macos")]
mod local_correction;
#[cfg(target_os = "macos")]
mod metrics;
#[cfg(target_os = "macos")]
mod rules;
#[cfg(target_os = "macos")]
mod runner;
#[cfg(target_os = "macos")]
mod wav;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Saymore evaluation failed: {error:#}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(target_os = "macos")]
fn run() -> Result<()> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    let Some(command) = arguments.first().map(String::as_str) else {
        bail!("usage: saymore-eval providers|run [options]");
    };
    match command {
        "providers" => {
            let environment = environment_option(&arguments[1..])?;
            runner::print_providers(environment)
        }
        "run" => {
            let request = required_path(&arguments[1..], "--request")?;
            let manifest = required_path(&arguments[1..], "--manifest")?;
            let recordings = required_path(&arguments[1..], "--recordings")?;
            let output = required_path(&arguments[1..], "--output")?;
            runner::run_evaluation(&request, &manifest, &recordings, &output)
        }
        other => bail!("unknown evaluation command: {other}"),
    }
}

#[cfg(not(target_os = "macos"))]
fn run() -> Result<()> {
    bail!("saymore-eval is supported only on macOS")
}

#[cfg(target_os = "macos")]
fn environment_option(arguments: &[String]) -> Result<AppEnvironment> {
    match option(arguments, "--environment").unwrap_or("development") {
        "development" => Ok(AppEnvironment::Development),
        "production" => Ok(AppEnvironment::Production),
        value => bail!("unsupported environment: {value}"),
    }
}

#[cfg(target_os = "macos")]
fn required_path(arguments: &[String], name: &str) -> Result<std::path::PathBuf> {
    option(arguments, name)
        .map(std::path::PathBuf::from)
        .with_context(|| format!("{name} is required"))
}

#[cfg(target_os = "macos")]
fn option<'a>(arguments: &'a [String], name: &str) -> Option<&'a str> {
    arguments
        .windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].as_str())
}
