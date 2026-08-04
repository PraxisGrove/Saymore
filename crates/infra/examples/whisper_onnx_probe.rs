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
use template_infra::{VerifiedModelInstaller, WhisperSpeechRecognizer};

const SAMPLE_RATE: i32 = 16_000;
const CHUNK_SAMPLES: usize = 9_600;

fn main() -> Result<(), Box<dyn Error>> {
    let arguments = Arguments::parse()?;
    let installer = VerifiedModelInstaller::whisper_large_v3_turbo(arguments.models_root)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let model = runtime.block_on(installer.install(Arc::new(|_| {})))?;
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
    println!("Audio: {}", arguments.wave.display());
    println!("Loading the pinned INT8 Whisper through the production adapter...");
    let load_started = Instant::now();
    let recognizer = WhisperSpeechRecognizer::load(&model)?;
    let load_elapsed = load_started.elapsed();
    println!("Load time: {:.2}s", load_elapsed.as_secs_f64());

    let first = transcribe_once(&recognizer, &samples, 1)?;
    let second = transcribe_once(&recognizer, &samples, 2)?;
    if compact(&first.text) != compact(&second.text) {
        return Err(io::Error::other(format!(
            "consecutive sessions disagreed: {:?} / {:?}",
            first.text, second.text
        ))
        .into());
    }
    if let Some(expected) = arguments.expected
        && compact(&first.text) != compact(&expected)
    {
        return Err(io::Error::other(format!(
            "unexpected transcript: expected {expected:?}, received {:?}",
            first.text
        ))
        .into());
    }

    println!("\nPASS: verified install and two production-adapter sessions completed");
    println!(
        "Summary: load {:.2}s, inference {:.2}s / {:.2}s",
        load_elapsed.as_secs_f64(),
        first.elapsed.as_secs_f64(),
        second.elapsed.as_secs_f64()
    );
    Ok(())
}

struct Arguments {
    models_root: PathBuf,
    wave: PathBuf,
    expected: Option<String>,
}

impl Arguments {
    fn parse() -> Result<Self, io::Error> {
        let mut arguments = env::args_os().skip(1);
        let models_root = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
        let wave = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
        let expected = arguments
            .next()
            .map(|value| value.into_string().map_err(|_| usage()))
            .transpose()?;
        if arguments.next().is_some() {
            return Err(usage());
        }
        Ok(Self {
            models_root,
            wave,
            expected,
        })
    }
}

struct Transcription {
    text: String,
    elapsed: Duration,
}

fn transcribe_once(
    recognizer: &WhisperSpeechRecognizer,
    samples: &[i16],
    run_number: usize,
) -> Result<Transcription, Box<dyn Error>> {
    println!(
        "\nRun {run_number}: {:.2}s, {} input chunks",
        samples.len() as f64 / f64::from(SAMPLE_RATE),
        samples.len().div_ceil(CHUNK_SAMPLES)
    );
    let session = recognizer.start(SpeechRecognitionHints::default(), Arc::new(|_| {}))?;
    let started = Instant::now();
    for chunk in samples.chunks(CHUNK_SAMPLES) {
        session.push_audio(chunk.to_vec())?;
    }
    let text = session.finish()?;
    let elapsed = started.elapsed();
    println!("  final: {text}");
    println!("  inference: {:.2}s", elapsed.as_secs_f64());
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
        "usage: whisper_onnx_probe MODELS_ROOT WAVE_PATH [EXPECTED_TEXT]",
    )
}
