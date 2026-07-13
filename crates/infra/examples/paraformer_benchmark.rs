use std::{
    env,
    error::Error,
    fs::{self, File},
    io::{self, BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    time::Instant,
};

use serde::Serialize;
use sherpa_onnx::{OnlineRecognizer, OnlineRecognizerConfig, Wave};

const CHUNK_SAMPLES: usize = 9_600;
const TAIL_PADDING_SAMPLES: usize = 4_800;

fn main() -> Result<(), Box<dyn Error>> {
    let arguments = Arguments::parse()?;
    let samples = load_manifest(&arguments.manifest, arguments.limit)?;
    println!(
        "Dataset: {} ({} samples)",
        arguments.manifest.display(),
        samples.len()
    );
    println!("Runtime: {}", arguments.variant.label());

    let load_started = Instant::now();
    let recognizer = create_recognizer(&arguments.model_dir, arguments.variant)?;
    let load_seconds = load_started.elapsed().as_secs_f64();
    println!("Model loaded in {load_seconds:.2}s");

    let evaluation = evaluate(&recognizer, &samples);
    let successful = samples.len().saturating_sub(evaluation.failures);
    let report = BenchmarkReport {
        runtime: arguments.variant.label(),
        manifest: arguments.manifest.display().to_string(),
        samples: samples.len(),
        successful,
        failures: evaluation.failures,
        exact_matches: evaluation.exact_matches,
        exact_match_rate: ratio(evaluation.exact_matches, successful),
        reference_characters: evaluation.reference_characters,
        character_errors: evaluation.character_errors,
        cer: ratio(evaluation.character_errors, evaluation.reference_characters),
        model_load_seconds: load_seconds,
        audio_seconds: evaluation.audio_seconds,
        inference_seconds: evaluation.inference_seconds,
        real_time_factor: float_ratio(evaluation.inference_seconds, evaluation.audio_seconds),
    };
    write_results(&arguments.output, &report, &evaluation.predictions)?;
    println!("\n{}", serde_json::to_string_pretty(&report)?);
    println!("Results: {}", arguments.output.display());
    Ok(())
}

fn evaluate(recognizer: &OnlineRecognizer, samples: &[Sample]) -> Evaluation {
    let mut predictions = Vec::with_capacity(samples.len());
    let mut total_audio_seconds = 0.0;
    let mut total_inference_seconds = 0.0;
    let mut total_reference_characters = 0;
    let mut total_character_errors = 0;
    let mut exact_matches = 0;
    let mut failures = 0;

    for (index, sample) in samples.iter().enumerate() {
        match transcribe(recognizer, &sample.audio) {
            Ok(transcription) => {
                let reference = normalize(&sample.reference);
                let hypothesis = normalize(&transcription.text);
                let errors = levenshtein(&reference, &hypothesis);
                total_audio_seconds += transcription.audio_seconds;
                total_inference_seconds += transcription.inference_seconds;
                total_reference_characters += reference.chars().count();
                total_character_errors += errors;
                exact_matches += usize::from(reference == hypothesis);
                predictions.push(Prediction {
                    audio: sample.audio.display().to_string(),
                    reference: sample.reference.clone(),
                    hypothesis: transcription.text,
                    reference_characters: reference.chars().count(),
                    character_errors: errors,
                    audio_seconds: transcription.audio_seconds,
                    inference_seconds: transcription.inference_seconds,
                    error: None,
                });
            }
            Err(error) => {
                failures += 1;
                let reference_characters = normalize(&sample.reference).chars().count();
                total_reference_characters += reference_characters;
                total_character_errors += reference_characters;
                predictions.push(Prediction {
                    audio: sample.audio.display().to_string(),
                    reference: sample.reference.clone(),
                    hypothesis: String::new(),
                    reference_characters,
                    character_errors: reference_characters,
                    audio_seconds: 0.0,
                    inference_seconds: 0.0,
                    error: Some(error.to_string()),
                });
            }
        }

        let completed = index + 1;
        if completed % 100 == 0 || completed == samples.len() {
            let cer = ratio(total_character_errors, total_reference_characters);
            println!(
                "Progress: {completed}/{} CER={:.2}% failures={failures}",
                samples.len(),
                cer * 100.0
            );
        }
    }

    Evaluation {
        predictions,
        failures,
        exact_matches,
        reference_characters: total_reference_characters,
        character_errors: total_character_errors,
        audio_seconds: total_audio_seconds,
        inference_seconds: total_inference_seconds,
    }
}

struct Evaluation {
    predictions: Vec<Prediction>,
    failures: usize,
    exact_matches: usize,
    reference_characters: usize,
    character_errors: usize,
    audio_seconds: f64,
    inference_seconds: f64,
}

#[derive(Clone, Copy)]
enum ModelVariant {
    Fp32,
    Q8,
}

impl ModelVariant {
    fn parse(value: &str) -> Result<Self, io::Error> {
        match value {
            "fp32" => Ok(Self::Fp32),
            "q8" => Ok(Self::Q8),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unknown model variant {value:?}; expected fp32 or q8"),
            )),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Fp32 => "fp32",
            Self::Q8 => "q8",
        }
    }

    fn encoder(self) -> &'static str {
        match self {
            Self::Fp32 => "encoder.fp32.onnx",
            Self::Q8 => "encoder.int8.onnx",
        }
    }

    fn decoder(self) -> &'static str {
        match self {
            Self::Fp32 => "decoder.fp32.onnx",
            Self::Q8 => "decoder.int8.onnx",
        }
    }
}

struct Arguments {
    model_dir: PathBuf,
    manifest: PathBuf,
    variant: ModelVariant,
    output: PathBuf,
    limit: Option<usize>,
}

impl Arguments {
    fn parse() -> Result<Self, Box<dyn Error>> {
        let values: Vec<String> = env::args().skip(1).collect();
        if !(4..=5).contains(&values.len()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "usage: paraformer_benchmark MODEL_DIR MANIFEST (fp32|q8) OUTPUT_JSON [LIMIT]",
            )
            .into());
        }
        let limit = values.get(4).map(|value| value.parse()).transpose()?;
        Ok(Self {
            model_dir: PathBuf::from(&values[0]),
            manifest: PathBuf::from(&values[1]),
            variant: ModelVariant::parse(&values[2])?,
            output: PathBuf::from(&values[3]),
            limit,
        })
    }
}

struct Sample {
    audio: PathBuf,
    reference: String,
}

fn load_manifest(path: &Path, limit: Option<usize>) -> Result<Vec<Sample>, Box<dyn Error>> {
    let file = BufReader::new(File::open(path)?);
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    let mut samples = Vec::new();
    for (index, line) in file.lines().enumerate() {
        let line = line?;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (audio, reference) = line.split_once('\t').ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("manifest line {} has no tab separator", index + 1),
            )
        })?;
        let audio = PathBuf::from(audio);
        samples.push(Sample {
            audio: if audio.is_absolute() {
                audio
            } else {
                base.join(audio)
            },
            reference: reference.to_owned(),
        });
        if samples.len() == limit.unwrap_or(usize::MAX) {
            break;
        }
    }
    if samples.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "manifest is empty").into());
    }
    Ok(samples)
}

fn create_recognizer(
    model_dir: &Path,
    variant: ModelVariant,
) -> Result<OnlineRecognizer, Box<dyn Error>> {
    let mut config = OnlineRecognizerConfig::default();
    config.model_config.paraformer.encoder = Some(path_owned(&model_dir.join(variant.encoder()))?);
    config.model_config.paraformer.decoder = Some(path_owned(&model_dir.join(variant.decoder()))?);
    config.model_config.tokens = Some(path_owned(&model_dir.join("tokens.txt"))?);
    config.model_config.num_threads = 4;
    config.model_config.provider = Some("cpu".to_owned());
    config.decoding_method = Some("greedy_search".to_owned());
    OnlineRecognizer::create(&config)
        .ok_or_else(|| io::Error::other("failed to create Paraformer recognizer").into())
}

struct Transcription {
    text: String,
    audio_seconds: f64,
    inference_seconds: f64,
}

fn transcribe(
    recognizer: &OnlineRecognizer,
    audio: &Path,
) -> Result<Transcription, Box<dyn Error>> {
    let wave = Wave::read(path_text(audio)?).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to read {}", audio.display()),
        )
    })?;
    let audio_seconds = wave.samples().len() as f64 / f64::from(wave.sample_rate());
    let stream = recognizer.create_stream();
    let started = Instant::now();
    for chunk in wave.samples().chunks(CHUNK_SAMPLES) {
        stream.accept_waveform(wave.sample_rate(), chunk);
        decode_ready(recognizer, &stream);
    }
    stream.accept_waveform(wave.sample_rate(), &[0.0; TAIL_PADDING_SAMPLES]);
    stream.set_option("is_final", "1");
    stream.input_finished();
    decode_ready(recognizer, &stream);
    let text = recognizer
        .get_result(&stream)
        .ok_or_else(|| io::Error::other("runtime returned no final result"))?
        .text;
    Ok(Transcription {
        text,
        audio_seconds,
        inference_seconds: started.elapsed().as_secs_f64(),
    })
}

fn decode_ready(recognizer: &OnlineRecognizer, stream: &sherpa_onnx::OnlineStream) {
    while recognizer.is_ready(stream) {
        recognizer.decode(stream);
    }
}

fn normalize(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .filter(|character| character.is_alphanumeric())
        .collect()
}

fn levenshtein(reference: &str, hypothesis: &str) -> usize {
    let reference: Vec<char> = reference.chars().collect();
    let hypothesis: Vec<char> = hypothesis.chars().collect();
    let mut previous: Vec<usize> = (0..=hypothesis.len()).collect();
    let mut current = vec![0; hypothesis.len() + 1];
    for (row, reference_character) in reference.iter().enumerate() {
        current[0] = row + 1;
        for (column, hypothesis_character) in hypothesis.iter().enumerate() {
            current[column + 1] = (previous[column + 1] + 1)
                .min(current[column] + 1)
                .min(previous[column] + usize::from(reference_character != hypothesis_character));
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[hypothesis.len()]
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn float_ratio(numerator: f64, denominator: f64) -> f64 {
    if denominator == 0.0 {
        0.0
    } else {
        numerator / denominator
    }
}

#[derive(Serialize)]
struct BenchmarkReport {
    runtime: &'static str,
    manifest: String,
    samples: usize,
    successful: usize,
    failures: usize,
    exact_matches: usize,
    exact_match_rate: f64,
    reference_characters: usize,
    character_errors: usize,
    cer: f64,
    model_load_seconds: f64,
    audio_seconds: f64,
    inference_seconds: f64,
    real_time_factor: f64,
}

#[derive(Serialize)]
struct Prediction {
    audio: String,
    reference: String,
    hypothesis: String,
    reference_characters: usize,
    character_errors: usize,
    audio_seconds: f64,
    inference_seconds: f64,
    error: Option<String>,
}

fn write_results(
    output: &Path,
    report: &BenchmarkReport,
    predictions: &[Prediction],
) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(output, serde_json::to_vec_pretty(report)?)?;
    let predictions_path = output.with_extension("predictions.jsonl");
    let mut writer = BufWriter::new(File::create(predictions_path)?);
    for prediction in predictions {
        serde_json::to_writer(&mut writer, prediction)?;
        writer.write_all(b"\n")?;
    }
    writer.flush()?;
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::{levenshtein, normalize};

    #[test]
    fn normalizes_case_spacing_and_punctuation() {
        assert_eq!("你好saymore2026", normalize("你好， SayMore 2026！"));
    }

    #[test]
    fn computes_character_edit_distance() {
        assert_eq!(1, levenshtein("语音识别", "语音别"));
        assert_eq!(1, levenshtein("语音", "语义"));
        assert_eq!(1, levenshtein("语音", "语音稿"));
    }
}
