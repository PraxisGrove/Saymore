use std::{env, process::ExitCode};

use anyhow::{Context, Result, bail};
use template_infra::AppEnvironment;

mod local_correction;
mod metrics;
mod rules;
mod runner;
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

fn environment_option(arguments: &[String]) -> Result<AppEnvironment> {
    match option(arguments, "--environment").unwrap_or("development") {
        "development" => Ok(AppEnvironment::Development),
        "production" => Ok(AppEnvironment::Production),
        value => bail!("unsupported environment: {value}"),
    }
}

fn required_path(arguments: &[String], name: &str) -> Result<std::path::PathBuf> {
    option(arguments, name)
        .map(std::path::PathBuf::from)
        .with_context(|| format!("{name} is required"))
}

fn option<'a>(arguments: &'a [String], name: &str) -> Option<&'a str> {
    arguments
        .windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].as_str())
}
