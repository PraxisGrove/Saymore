use std::{fs, path::PathBuf, process::ExitCode};

use template_infra::run_macos_speech_probe;

const COMMAND: &str = "--probe-apple-speech";
const STANDARD_AUDIO: &[u8] = include_bytes!("../assets/asr-test/standard-zh.pcm");

pub(crate) fn run_if_requested() -> Option<ExitCode> {
    (std::env::args().nth(1).as_deref() == Some(COMMAND)).then(run)
}

fn run() -> ExitCode {
    match output_path()
        .and_then(|path| standard_audio_samples().map(|samples| (path, samples)))
        .and_then(|(path, samples)| write_report(path, &samples))
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Apple Speech probe failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn output_path() -> Result<PathBuf, String> {
    std::env::args()
        .nth(2)
        .map(PathBuf::from)
        .ok_or_else(|| "the probe requires an output JSON path".to_owned())
}

fn standard_audio_samples() -> Result<Vec<i16>, String> {
    if !STANDARD_AUDIO.len().is_multiple_of(2) {
        return Err("the bundled probe audio has an invalid byte length".to_owned());
    }
    Ok(STANDARD_AUDIO
        .chunks_exact(2)
        .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]))
        .collect())
}

fn write_report(path: PathBuf, samples: &[i16]) -> Result<(), String> {
    let report = run_macos_speech_probe(samples).map_err(|error| error.to_string())?;
    let json = serde_json::to_vec_pretty(&report).map_err(|error| error.to_string())?;
    fs::write(path, json).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_audio_is_valid_pcm() {
        let samples = standard_audio_samples();
        assert!(samples.is_ok());
        assert_eq!(Some(49_536), samples.ok().map(|samples| samples.len()));
    }
}
