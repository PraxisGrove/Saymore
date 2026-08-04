use std::{error::Error, io, path::PathBuf};

use template_infra::OfflinePunctuationRestorer;

fn main() -> Result<(), Box<dyn Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let model_directory = arguments
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing model directory"))?;
    let text = arguments
        .next()
        .and_then(|value| value.into_string().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing input text"))?;
    let punctuation = OfflinePunctuationRestorer::load(&model_directory)?;
    println!("{}", punctuation.add_punctuation(&text)?);
    Ok(())
}
