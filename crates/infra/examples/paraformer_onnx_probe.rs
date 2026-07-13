use std::{
    env,
    error::Error,
    io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use sherpa_onnx::{OnlineRecognizer, OnlineRecognizerConfig, Wave};

const SAMPLE_RATE: i32 = 16_000;
const CHUNK_SAMPLES: usize = 9_600;
const TAIL_PADDING_SAMPLES: usize = 4_800;
const EXPECTED_TEXT: &str = "欢迎大家来体验达摩院推出的语音识别模型";

fn main() -> Result<(), Box<dyn Error>> {
    let model_dir = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("models/paraformer-zh-streaming"));
    let wave_path = model_dir.join("example/asr_example.wav");
    let wave = Wave::read(path_text(&wave_path)?).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to read {}", wave_path.display()),
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

    println!("Model: {}", model_dir.display());
    println!("Loading Q8 Paraformer once on CPU...");
    let load_started = Instant::now();
    let recognizer = create_recognizer(&model_dir)?;
    let load_elapsed = load_started.elapsed();
    println!("Load time: {:.2}s", load_elapsed.as_secs_f64());

    let first = transcribe_once(&recognizer, &wave, 1)?;
    let second = transcribe_once(&recognizer, &wave, 2)?;
    for (run_number, result) in [(1, &first), (2, &second)] {
        if compact(&result.text) != EXPECTED_TEXT {
            return Err(io::Error::other(format!(
                "run {run_number} returned unexpected text: {}",
                result.text
            ))
            .into());
        }
    }

    println!("\nPASS: one native model load completed two streaming transcriptions");
    println!(
        "Summary: load {:.2}s, inference {:.2}s / {:.2}s",
        load_elapsed.as_secs_f64(),
        first.elapsed.as_secs_f64(),
        second.elapsed.as_secs_f64()
    );
    Ok(())
}

fn create_recognizer(model_dir: &Path) -> Result<OnlineRecognizer, Box<dyn Error>> {
    let mut config = OnlineRecognizerConfig::default();
    config.model_config.paraformer.encoder =
        Some(path_owned(&model_dir.join("encoder.int8.onnx"))?);
    config.model_config.paraformer.decoder =
        Some(path_owned(&model_dir.join("decoder.int8.onnx"))?);
    config.model_config.tokens = Some(path_owned(&model_dir.join("tokens.txt"))?);
    config.model_config.num_threads = 4;
    config.model_config.provider = Some("cpu".to_owned());
    config.decoding_method = Some("greedy_search".to_owned());

    OnlineRecognizer::create(&config)
        .ok_or_else(|| io::Error::other("failed to create native Paraformer recognizer").into())
}

struct Transcription {
    text: String,
    elapsed: Duration,
}

fn transcribe_once(
    recognizer: &OnlineRecognizer,
    wave: &Wave,
    run_number: usize,
) -> Result<Transcription, Box<dyn Error>> {
    let stream = recognizer.create_stream();
    let started = Instant::now();
    let mut last_partial = String::new();

    println!(
        "\nRun {run_number}: {:.2}s, {} chunks",
        wave.samples().len() as f64 / f64::from(wave.sample_rate()),
        wave.samples().len().div_ceil(CHUNK_SAMPLES)
    );
    for chunk in wave.samples().chunks(CHUNK_SAMPLES) {
        stream.accept_waveform(wave.sample_rate(), chunk);
        decode_ready(recognizer, &stream);
        print_changed_partial(recognizer, &stream, &mut last_partial);
    }

    stream.accept_waveform(wave.sample_rate(), &vec![0.0; TAIL_PADDING_SAMPLES]);
    stream.set_option("is_final", "1");
    stream.input_finished();
    decode_ready(recognizer, &stream);
    let text = recognizer
        .get_result(&stream)
        .ok_or_else(|| io::Error::other("native runtime did not return a final result"))?
        .text;
    let elapsed = started.elapsed();
    println!("  final: {}", text.trim());
    println!("  inference: {:.2}s", elapsed.as_secs_f64());

    Ok(Transcription { text, elapsed })
}

fn decode_ready(recognizer: &OnlineRecognizer, stream: &sherpa_onnx::OnlineStream) {
    while recognizer.is_ready(stream) {
        recognizer.decode(stream);
    }
}

fn print_changed_partial(
    recognizer: &OnlineRecognizer,
    stream: &sherpa_onnx::OnlineStream,
    last_partial: &mut String,
) {
    if let Some(result) = recognizer.get_result(stream)
        && !result.text.is_empty()
        && result.text != *last_partial
    {
        println!("  partial: {}", result.text.trim());
        *last_partial = result.text;
    }
}

fn compact(text: &str) -> String {
    text.split_whitespace().collect()
}

fn path_text(path: &Path) -> Result<&str, io::Error> {
    path.to_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("path is not valid UTF-8: {}", path.display()),
        )
    })
}

fn path_owned(path: &Path) -> Result<String, io::Error> {
    path_text(path).map(ToOwned::to_owned)
}
