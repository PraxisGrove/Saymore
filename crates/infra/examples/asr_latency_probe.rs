//! Replays caller-supplied 16 kHz mono PCM at capture speed, without microphone
//! access or user settings. Reports stop-to-final latency separately from capture.
mod latency_support;

use std::{
    env,
    error::Error,
    fs, io,
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
use template_app::{SpeechRecognitionHints, StreamingSpeechRecognizer};
use template_infra::{SeacoSpeechRecognizer, WhisperSpeechRecognizer, ZipformerSpeechRecognizer};
fn main() -> Result<(), Box<dyn Error>> {
    init_diagnostics();
    let mut args = env::args_os().skip(1);
    let model = args
        .next()
        .and_then(|s| s.into_string().ok())
        .ok_or_else(usage)?;
    let directory = args.next().map(PathBuf::from).ok_or_else(usage)?;
    let pcm = args.next().map(PathBuf::from).ok_or_else(usage)?;
    let feed = args
        .next()
        .and_then(|s| s.into_string().ok())
        .ok_or_else(usage)?;
    let refinement = args
        .next()
        .and_then(|s| s.into_string().ok())
        .unwrap_or_else(|| "disabled".into());
    validate_refinement(&refinement)?;
    if !matches!(feed.as_str(), "capture" | "buffered") || args.next().is_some() {
        return Err(usage().into());
    }
    let samples = read_pcm(pcm)?;
    let load = Instant::now();
    let recognizer = load_recognizer(&model, &directory)?;
    let load_seconds = load.elapsed().as_secs_f64();
    // Warm the same decoder with a short prefix before measuring scheduling.
    let cold = env::var("SAYMORE_BENCH_COLD").is_ok_and(|v| v == "1");
    warm_up(recognizer.as_ref(), &samples, cold)?;
    let start = Instant::now();
    let first_partial = Arc::new(Mutex::new(None));
    let observed = Arc::clone(&first_partial);
    let session = recognizer.start(
        SpeechRecognitionHints::default(),
        Arc::new(move |_| {
            if let Ok(mut value) = observed.lock() {
                value.get_or_insert(start.elapsed().as_secs_f64());
            }
        }),
    )?;
    let scheduling =
        env::var("SAYMORE_BENCH_SCHEDULING").unwrap_or_else(|_| "during-capture".into());
    let session = latency_support::scheduling::ScheduledSession::new(session, &scheduling)?;
    let mut received = 0;
    for chunk in samples.chunks(1600) {
        received += chunk.len();
        let due = start + Duration::from_secs_f64(received as f64 / 16000.0);
        thread::sleep(due.saturating_duration_since(Instant::now()));
        if feed == "capture" {
            session.push_audio(chunk.to_vec())?;
        }
    }
    let stopped = Instant::now();
    if feed == "buffered" {
        session.push_audio(samples.clone())?;
    }
    let text = session.finish()?;
    let asr_seconds = stopped.elapsed().as_secs_f64();
    let asr_total_seconds = start.elapsed().as_secs_f64();
    let processed = latency_support::process(text.clone(), &refinement)?;
    let ready_seconds = stopped.elapsed().as_secs_f64();
    let first = first_observation(&first_partial)?;
    let measurement = serde_json::json!({
        "model": model, "feed": feed, "scheduling": scheduling,
        "qwen_mode": env::var("SAYMORE_BENCH_QWEN_MODE").unwrap_or_else(|_| "non_streaming".into()),
        "decoder_mode": decoder_mode(&model),
        "audio_seconds": samples.len() as f64 / 16000.0,
        "load_seconds": load_seconds,
        "stop_to_final_seconds": asr_seconds,
        "total_seconds": asr_total_seconds,
        "stop_to_text_ready_seconds": ready_seconds,
        "capture_to_text_ready_seconds": start.elapsed().as_secs_f64(),
        "cold": cold, "refinement": refinement, "processed": processed,
        "delivery": "not_measured",
        "first_partial_seconds": first,
        "text": text,
    });
    println!("{}", serde_json::to_string_pretty(&measurement)?);
    Ok(())
}

fn decoder_mode(model: &str) -> &'static str {
    if model == "zipformer" {
        "native_online"
    } else if model == "qwen3"
        && env::var("SAYMORE_BENCH_QWEN_MODE").is_ok_and(|value| value == "streaming")
    {
        "official_streaming_algorithm_port"
    } else {
        "offline_segments"
    }
}

fn init_diagnostics() {
    if env::var_os("SAYMORE_BENCH_DIAGNOSTICS").is_some() {
        use tracing_subscriber::prelude::*;
        tracing_subscriber::registry()
            .with(
                tracing_subscriber::fmt::layer()
                    .with_writer(io::stderr)
                    .with_ansi(false)
                    .with_filter(tracing_subscriber::filter::filter_fn(|metadata| {
                        metadata.target() == "saymore::diagnostics"
                    })),
            )
            .init();
    }
}

fn validate_refinement(value: &str) -> Result<(), io::Error> {
    match value {
        "disabled" | "light" | "standard" | "thorough" => Ok(()),
        _ => Err(usage()),
    }
}

fn first_observation(value: &Mutex<Option<f64>>) -> Result<Option<f64>, io::Error> {
    value
        .lock()
        .map(|value| *value)
        .map_err(|_| io::Error::other("partial observation lock failed"))
}

fn warm_up(
    recognizer: &dyn StreamingSpeechRecognizer,
    samples: &[i16],
    cold: bool,
) -> Result<(), Box<dyn Error>> {
    if !cold {
        let warm = recognizer.start(SpeechRecognitionHints::default(), Arc::new(|_| {}))?;
        warm.push_audio(samples[..samples.len().min(4 * 16000)].to_vec())?;
        warm.finish()?;
    }
    Ok(())
}

fn read_pcm(path: PathBuf) -> Result<Vec<i16>, Box<dyn Error>> {
    let bytes = fs::read(path)?;
    if bytes.is_empty() || !bytes.len().is_multiple_of(2) {
        return Err(usage().into());
    }
    Ok(bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| i16::from_le_bytes(*b))
        .collect())
}

fn load_recognizer(
    model: &str,
    directory: &std::path::Path,
) -> Result<Box<dyn StreamingSpeechRecognizer>, Box<dyn Error>> {
    Ok(match model {
        "zipformer" => Box::new(ZipformerSpeechRecognizer::load(directory)?),
        "whisper" => Box::new(WhisperSpeechRecognizer::load(directory)?),
        "qwen3" => Box::new(latency_support::qwen::QwenReplay::load(directory)?),
        "seaco" => Box::new(SeacoSpeechRecognizer::load(directory)?),
        _ => return Err(usage().into()),
    })
}

fn usage() -> io::Error {
    io::Error::other(
        "usage: asr_latency_probe whisper|qwen3|seaco|zipformer MODEL_DIR TEST_I16_PCM capture|buffered [disabled|light|standard|thorough]",
    )
}
