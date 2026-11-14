//! Runs only caller-supplied test audio through the production MOSS adapter.
use std::{env, error::Error, fs, io, path::PathBuf, sync::Arc, time::Instant};
use template_app::{SpeechRecognitionHints, StreamingSpeechRecognizer};
use template_infra::MossSpeechRecognizer;

fn main() -> Result<(), Box<dyn Error>> {
    let mut arguments = env::args_os().skip(1);
    let directory = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
    let audio = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
    let expected = arguments.next();
    if arguments.next().is_some() {
        return Err(usage().into());
    }
    let bytes = fs::read(audio)?;
    if bytes.is_empty() || !bytes.len().is_multiple_of(2) {
        return Err(usage().into());
    }
    let samples: Vec<_> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect();
    let started = Instant::now();
    let recognizer = MossSpeechRecognizer::load(&directory)?;
    println!("Load: {:.2}s", started.elapsed().as_secs_f64());
    for run in 1..=2 {
        let started = Instant::now();
        let session = recognizer.start(
            SpeechRecognitionHints::from_terms(vec!["人机交互".into(), "Saymore".into()]),
            Arc::new(|_| {}),
        )?;
        session.push_audio(samples.clone())?;
        let text = session.finish()?;
        println!("Run {run}: {:.2}s; {text}", started.elapsed().as_secs_f64());
        if let Some(expected) = expected.as_ref()
            && text.trim() != expected.to_string_lossy().trim()
        {
            return Err(io::Error::other("transcript differs from expected text").into());
        }
    }
    Ok(())
}

fn usage() -> io::Error {
    io::Error::other(
        "usage: moss_onnx_probe PINNED_MODEL_DIRECTORY TEST_16KHZ_MONO_I16_PCM [EXPECTED_TEXT]",
    )
}
