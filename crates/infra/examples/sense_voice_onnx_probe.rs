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
    SenseVoiceSpeechRecognizer, VerifiedModelInstaller, current_process_resident_memory_bytes,
};

mod probe_support;

const SAMPLE_RATE: i32 = 16_000;
const CHUNK_SAMPLES: usize = 9_600;

fn main() -> Result<(), Box<dyn Error>> {
    let arguments = Arguments::parse()?;
    let installer = VerifiedModelInstaller::sense_voice_small(arguments.models_root)?;
    let model = probe_support::install_model("SenseVoice", &installer, 10 * 1024 * 1024)?;
    let wave = Wave::read(path_text(&arguments.wave)?).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to read {}", arguments.wave.display()),
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
    let samples = pcm_i16(wave.samples());

    println!("Model: {}", model.display());
    println!(
        "Installed size: {} bytes",
        installer.installed_size_bytes()?
    );
    println!("Audio: {}", arguments.wave.display());
    let memory_before = current_process_resident_memory_bytes();
    let load_started = Instant::now();
    let recognizer = SenseVoiceSpeechRecognizer::load(&model)?;
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

    let first = transcribe_once(&recognizer, &samples, 1)?;
    let second = transcribe_once(&recognizer, &samples, 2)?;
    if compact(&first.text) != compact(&second.text) {
        return Err(io::Error::other(format!(
            "consecutive sessions disagreed: {:?} / {:?}",
            first.text, second.text
        ))
        .into());
    }

    println!("PASS: verified install and two production-adapter sessions completed");
    println!(
        "Summary: load {:.2}s, inference {:.2}s / {:.2}s, final {:?}",
        load_elapsed.as_secs_f64(),
        first.elapsed.as_secs_f64(),
        second.elapsed.as_secs_f64(),
        first.text
    );
    Ok(())
}

struct Arguments {
    models_root: PathBuf,
    wave: PathBuf,
}

impl Arguments {
    fn parse() -> Result<Self, io::Error> {
        let mut arguments = env::args_os().skip(1);
        let models_root = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
        let wave = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
        if arguments.next().is_some() {
            return Err(usage());
        }
        Ok(Self { models_root, wave })
    }
}

struct Transcription {
    text: String,
    elapsed: Duration,
}

fn transcribe_once(
    recognizer: &SenseVoiceSpeechRecognizer,
    samples: &[i16],
    run_number: usize,
) -> Result<Transcription, Box<dyn Error>> {
    let session = recognizer.start(SpeechRecognitionHints::default(), Arc::new(|_| {}))?;
    let started = Instant::now();
    for chunk in samples.chunks(CHUNK_SAMPLES) {
        session.push_audio(chunk.to_vec())?;
    }
    let text = session.finish()?;
    let elapsed = started.elapsed();
    println!("Run {run_number}: {text} ({:.2}s)", elapsed.as_secs_f64());
    Ok(Transcription { text, elapsed })
}

fn pcm_i16(samples: &[f32]) -> Vec<i16> {
    samples
        .iter()
        .map(|sample| (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16)
        .collect()
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
        "usage: sense_voice_onnx_probe MODELS_ROOT WAVE_PATH",
    )
}
