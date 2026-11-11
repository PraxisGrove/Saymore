use std::{env, error::Error, fs, io, path::Path, sync::Arc, time::Instant};

use sherpa_onnx::Wave;
use template_app::{SpeechRecognitionError, SpeechRecognitionHints, StreamingSpeechRecognizer};
use template_infra::{MoonshineSpeechRecognizer, ZipformerSpeechRecognizer};

fn main() -> Result<(), Box<dyn Error>> {
    let arguments = env::args_os().skip(1).collect::<Vec<_>>();
    let [
        zipformer_directory,
        moonshine_directory,
        chinese_pcm,
        english_wave,
        expected_english,
    ] = arguments.as_slice()
    else {
        return Err(io::Error::new(io::ErrorKind::InvalidInput,
            "usage: complementary_asr_probe ZIPFORMER_DIR MOONSHINE_DIR CHINESE_PCM_16KHZ ENGLISH_WAV_16KHZ EXPECTED_ENGLISH_PHRASE").into());
    };
    let (chinese, english) = read_audio(Path::new(chinese_pcm), Path::new(english_wave))?;
    let expected_english = expected_english
        .to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid expected English"))?;
    let started = Instant::now();
    let zipformer = ZipformerSpeechRecognizer::load(Path::new(zipformer_directory))?;
    println!(
        "Zipformer loaded in {:.2}s",
        started.elapsed().as_secs_f64()
    );
    probe(
        "Zipformer Chinese",
        &zipformer,
        &chinese,
        "可以通过语音进行人机交互",
        SpeechRecognitionHints::default(),
    )?;
    probe(
        "Zipformer Chinese hinted",
        &zipformer,
        &chinese,
        "可以通过语音进行人机交互",
        SpeechRecognitionHints::from_terms(vec!["语音识别".to_owned(), "Saymore".to_owned()]),
    )?;
    probe(
        "Zipformer English",
        &zipformer,
        &english,
        expected_english,
        SpeechRecognitionHints::default(),
    )?;
    let started = Instant::now();
    let moonshine = MoonshineSpeechRecognizer::load(Path::new(moonshine_directory))?;
    println!(
        "Moonshine loaded in {:.2}s",
        started.elapsed().as_secs_f64()
    );
    probe(
        "Moonshine English",
        &moonshine,
        &english,
        expected_english,
        SpeechRecognitionHints::default(),
    )?;
    println!(
        "PASS: native adapters, consecutive sessions, chunk invariance, empty input and cancellation"
    );
    Ok(())
}

fn probe(
    label: &str,
    recognizer: &dyn StreamingSpeechRecognizer,
    samples: &[i16],
    expected_fragment: &str,
    hints: SpeechRecognitionHints,
) -> Result<(), Box<dyn Error>> {
    let mut previous = None;
    for chunk_size in [1_600, 9_600] {
        let session = recognizer.start(hints.clone(), Arc::new(|_| {}))?;
        let started = Instant::now();
        for chunk in samples.chunks(chunk_size) {
            session.push_audio(chunk.to_vec())?;
        }
        let text = session.finish()?;
        println!(
            "{label} ({chunk_size} samples/chunk): {text} [{:.2}s]",
            started.elapsed().as_secs_f64()
        );
        if !compact(&text).contains(&compact(expected_fragment)) {
            return Err(
                io::Error::other(format!("{label}: missing expected fixture phrase")).into(),
            );
        }
        if previous.as_ref().is_some_and(|previous| previous != &text) {
            return Err(
                io::Error::other(format!("{label}: chunk boundaries changed the result")).into(),
            );
        }
        previous = Some(text);
    }
    let empty = recognizer.start(SpeechRecognitionHints::default(), Arc::new(|_| {}))?;
    empty.push_audio(Vec::new())?;
    if !matches!(empty.finish(), Err(SpeechRecognitionError::Protocol(_))) {
        return Err(io::Error::other(format!("{label}: empty audio was accepted")).into());
    }
    let cancelled = recognizer.start(hints, Arc::new(|_| {}))?;
    cancelled.push_audio(samples[..samples.len().min(1_600)].to_vec())?;
    cancelled.cancel();
    Ok(())
}

fn read_audio(
    chinese_pcm: &Path,
    english_wave: &Path,
) -> Result<(Vec<i16>, Vec<i16>), Box<dyn Error>> {
    let bytes = fs::read(chinese_pcm)?;
    if bytes.len() % 2 != 0 {
        return Err(
            io::Error::new(io::ErrorKind::InvalidData, "incomplete Chinese PCM sample").into(),
        );
    }
    let chinese = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|chunk| i16::from_le_bytes(*chunk))
        .collect();
    let wave_path = english_wave
        .to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid wave path"))?;
    let wave = Wave::read(wave_path)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "could not read English wave"))?;
    if wave.sample_rate() != 16_000 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "expected mono 16 kHz English wave",
        )
        .into());
    }
    let english = wave
        .samples()
        .iter()
        .map(|sample| (sample.clamp(-1.0, 1.0) * 32_768.0).round() as i16)
        .collect();
    Ok((chinese, english))
}

fn compact(text: &str) -> String {
    text.chars()
        .filter(|ch| ch.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}
