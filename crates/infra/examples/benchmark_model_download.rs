//! Downloads benchmark weights using the production pinned/hash-verified installer.
use std::{env, error::Error, io, path::PathBuf, sync::Arc};
use template_infra::VerifiedModelInstaller;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args().skip(1);
    let model = args
        .next()
        .ok_or_else(|| io::Error::other("missing model"))?;
    let root = PathBuf::from(
        args.next()
            .ok_or_else(|| io::Error::other("missing destination"))?,
    );
    let installer = match model.as_str() {
        "whisper" => VerifiedModelInstaller::whisper_large_v3_turbo(root)?,
        "qwen3" => VerifiedModelInstaller::qwen3_asr_1_7b(root)?,
        "zipformer" => VerifiedModelInstaller::zipformer(root)?,
        _ => return Err(io::Error::other("supported downloads: whisper, qwen3, zipformer").into()),
    };
    installer.ensure_install_space()?;
    eprintln!("model={model} bytes={}", installer.download_size_bytes());
    let directory = installer.install(Arc::new(|_| {})).await?;
    println!("{}", directory.display());
    Ok(())
}
