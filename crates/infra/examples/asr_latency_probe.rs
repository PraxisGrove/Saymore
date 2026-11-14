//! Replays caller-supplied 16 kHz mono PCM at capture speed, without microphone
//! access or user settings. Reports stop-to-final latency separately from capture.
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
use template_infra::{
    MossSpeechRecognizer, Qwen3AsrSpeechRecognizer, SeacoSpeechRecognizer, WhisperSpeechRecognizer,
};

fn main() -> Result<(), Box<dyn Error>> {
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
    if !matches!(feed.as_str(), "capture" | "buffered") || args.next().is_some() {
        return Err(usage().into());
    }
    let bytes = fs::read(pcm)?;
    if bytes.is_empty() || !bytes.len().is_multiple_of(2) {
        return Err(usage().into());
    }
    let samples: Vec<_> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| i16::from_le_bytes(*b))
        .collect();
    let load = Instant::now();
    let recognizer: Box<dyn StreamingSpeechRecognizer> = match model.as_str() {
        "moss" => Box::new(MossSpeechRecognizer::load(&directory)?),
        "whisper" => Box::new(WhisperSpeechRecognizer::load(&directory)?),
        "qwen3" => Box::new(Qwen3AsrSpeechRecognizer::load(&directory)?),
        "seaco" => Box::new(SeacoSpeechRecognizer::load(&directory)?),
        _ => return Err(usage().into()),
    };
    let load_seconds = load.elapsed().as_secs_f64();
    // Warm the same decoder with a short prefix before measuring scheduling.
    let warm = recognizer.start(SpeechRecognitionHints::default(), Arc::new(|_| {}))?;
    warm.push_audio(samples[..samples.len().min(4 * 16000)].to_vec())?;
    warm.finish()?;
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
    let first = *first_partial
        .lock()
        .map_err(|_| io::Error::other("partial observation lock failed"))?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "model": model, "feed": feed,
            "audio_seconds": samples.len() as f64 / 16000.0,
            "load_seconds": load_seconds,
            "stop_to_final_seconds": stopped.elapsed().as_secs_f64(),
            "total_seconds": start.elapsed().as_secs_f64(),
            "first_partial_seconds": first,
            "text": text,
        }))?
    );
    Ok(())
}

fn usage() -> io::Error {
    io::Error::other(
        "usage: asr_latency_probe moss|whisper|qwen3|seaco MODEL_DIR TEST_I16_PCM capture|buffered",
    )
}
