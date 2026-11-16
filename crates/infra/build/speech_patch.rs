use sha2::{Digest, Sha256};
use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

/// Recreate the verified upstream tree when the local continuation patch changes.
pub(super) fn prepare(out: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let patch = Path::new(env!("CARGO_MANIFEST_DIR")).join("native/qwen-streaming.patch");
    let fingerprint = format!("{:x}", Sha256::digest(fs::read(&patch)?));
    let directory = out.join(format!("speech.cpp-{}", super::SPEECH_REVISION));
    let marker = directory.join(".saymore-streaming-patch");
    let patched = fs::read_to_string(directory.join("include/speech.h"))
        .is_ok_and(|text| text.contains("SPEECH_OPT_PREFIX"));
    if directory.exists()
        && (!patched || fs::read_to_string(&marker).unwrap_or_default() != fingerprint)
    {
        fs::remove_dir_all(&directory)?;
    }
    let source = super::unpack_source(
        out,
        "speech.cpp",
        super::SPEECH_REVISION,
        "cf233c7b44eec7b1a91c299fd7ad5b67930ec46e1e913f970fd86bc2ea13cafa",
    )?;
    if !marker.exists() {
        // --no-index applies only to this extracted build tree, not Git's index.
        let status = Command::new("git")
            .current_dir(&source)
            .env("GIT_CEILING_DIRECTORIES", out)
            .args(["apply", "--no-index", "--whitespace=nowarn"])
            .arg(&patch)
            .status()?;
        if !status.success()
            || !fs::read_to_string(source.join("include/speech.h"))?.contains("SPEECH_OPT_PREFIX")
        {
            return Err("native Qwen continuation patch failed".into());
        }
        fs::write(marker, fingerprint)?;
    }
    Ok(source)
}
