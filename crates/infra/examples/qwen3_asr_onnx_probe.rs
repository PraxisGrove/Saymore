use std::{
    env,
    error::Error,
    io,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use sherpa_onnx::Wave;
use template_app::{SpeechRecognitionHints, StreamingSpeechRecognizer};
use template_infra::{
    Qwen3AsrSpeechRecognizer, VerifiedModelInstaller, current_process_resident_memory_bytes,
};

mod probe_support;

const SAMPLE_RATE: i32 = 16_000;

fn main() -> Result<(), Box<dyn Error>> {
    let (models_root, wave_path, maximum_seconds) = arguments()?;
    let installer = VerifiedModelInstaller::qwen3_asr_1_7b(models_root)?;
    let model = probe_support::install_model("Qwen3-ASR", &installer, 100 * 1024 * 1024)?;
    let mut samples = read_samples(&wave_path)?;
    if let Some(maximum_seconds) = maximum_seconds {
        samples.truncate(maximum_seconds.saturating_mul(SAMPLE_RATE as usize));
    }
    println!("Model: {}", model.display());
    println!(
        "Installed size: {} bytes",
        installer.installed_size_bytes()?
    );
    let memory_before = current_process_resident_memory_bytes();
    let load_started = Instant::now();
    let recognizer = Qwen3AsrSpeechRecognizer::load(&model)?;
    let load_elapsed = load_started.elapsed();
    let memory_after = current_process_resident_memory_bytes();
    println!("Load time: {:.2}s", load_elapsed.as_secs_f64());
    println!(
        "Resident memory: before {:?}, after {:?}, delta {:?} bytes",
        memory_before,
        memory_after,
        memory_before
            .zip(memory_after)
            .and_then(|(before, after)| after.checked_sub(before))
    );
    let first = transcribe(&recognizer, &samples)?;
    let second = transcribe(&recognizer, &samples)?;
    if compact(&first.text) != compact(&second.text) {
        return Err(io::Error::other(format!(
            "consecutive sessions disagreed: {:?} / {:?}",
            first.text, second.text
        ))
        .into());
    }
    println!(
        "PASS: two sessions returned {:?}; inference {:.2}s / {:.2}s",
        first.text,
        first.elapsed.as_secs_f64(),
        second.elapsed.as_secs_f64()
    );
    Ok(())
}

fn read_samples(path: &Path) -> Result<Vec<i16>, Box<dyn Error>> {
    if path.extension().is_some_and(|extension| extension == "pcm") {
        let bytes = std::fs::read(path)?;
        if !bytes.len().is_multiple_of(2) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "odd PCM byte count").into());
        }
        return Ok(bytes
            .chunks_exact(2)
            .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]))
            .collect());
    }
    let wave = Wave::read(path_text(path)?).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to read {}", path.display()),
        )
    })?;
    if wave.sample_rate() != SAMPLE_RATE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "expected {SAMPLE_RATE} Hz audio, found {} Hz",
                wave.sample_rate()
            ),
        )
        .into());
    }
    Ok(wave
        .samples()
        .iter()
        .map(|sample| (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16)
        .collect())
}

struct Transcription {
    text: String,
    elapsed: Duration,
}

fn transcribe(
    recognizer: &Qwen3AsrSpeechRecognizer,
    samples: &[i16],
) -> Result<Transcription, Box<dyn Error>> {
    let session = recognizer.start(SpeechRecognitionHints::default(), Arc::new(|_| {}))?;
    let started = Instant::now();
    for chunk in samples.chunks(1_600) {
        session.push_audio(chunk.to_vec())?;
    }
    let text = session.finish()?;
    let elapsed = started.elapsed();
    println!("Inference: {:.2}s; final: {text}", elapsed.as_secs_f64());
    Ok(Transcription { text, elapsed })
}

fn arguments() -> Result<(PathBuf, PathBuf, Option<usize>), io::Error> {
    let mut values = env::args_os().skip(1).map(PathBuf::from);
    let models = values.next().ok_or_else(usage)?;
    let wave = values.next().ok_or_else(usage)?;
    let maximum_seconds = values
        .next()
        .map(|value| {
            value
                .to_str()
                .and_then(|value| value.parse::<usize>().ok())
                .filter(|value| *value > 0)
                .ok_or_else(usage)
        })
        .transpose()?;
    if values.next().is_some() {
        return Err(usage());
    }
    Ok((models, wave, maximum_seconds))
}

fn compact(text: &str) -> String {
    text.split_whitespace().collect()
}

fn path_text(path: &Path) -> Result<&str, io::Error> {
    path.to_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("path is not valid Unicode: {}", path.display()),
        )
    })
}

fn usage() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "usage: qwen3_asr_onnx_probe MODELS_ROOT WAVE_PATH [MAX_SECONDS]",
    )
}
