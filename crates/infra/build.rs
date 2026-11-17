//! Build the pinned, isolated native Qwen worker. It is packaged beside the app.
use sha2::{Digest, Sha256};
use std::{
    env,
    error::Error,
    fs, io,
    path::{Path, PathBuf},
    time::Duration,
};

#[path = "build/native_sources.rs"]
mod native_sources;
#[path = "build/speech_patch.rs"]
mod speech_patch;

const SPEECH_REVISION: &str = "c4bdcd0e751bd29460cb1353e6d54499828a1d90";
const GGML_REVISION: &str = "cbdeccfd49ec9c702c175c273d36c47520cf4710";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build/speech_patch.rs");
    println!("cargo:rerun-if-changed=build/native_sources.rs");
    println!("cargo:rerun-if-changed=native/qwen-streaming.patch");
    if let Err(error) = build_worker() {
        eprintln!("could not build the pinned Qwen speech.cpp worker: {error}");
        std::process::exit(1);
    }
}

fn build_worker() -> Result<(), Box<dyn Error>> {
    let out = PathBuf::from(env::var_os("OUT_DIR").ok_or("missing OUT_DIR")?);
    let source = speech_patch::prepare(&out)
        .map_err(|error| format!("prepare speech.cpp in {}: {error}", out.display()))?;
    let ggml = unpack_source(
        &out,
        "ggml",
        GGML_REVISION,
        "03f751738259e0c0a8a6a98b15934afee8e6768aa21c815f75eb4a5a2ad40b50",
    )
    .map_err(|error| format!("unpack ggml in {}: {error}", out.display()))?;
    // Source archives do not contain the submodule. Copy only the pinned tree.
    let submodule = source.join("ggml");
    native_sources::stage(&ggml, &submodule, GGML_REVISION)?;
    let target = env::var("TARGET")?;
    let macos = target.contains("apple-darwin");
    let metal = macos && target.starts_with("aarch64");
    let mut config = cmake::Config::new(&source);
    config
        .out_dir(out.join("native"))
        .profile("Release")
        .build_target("speech-cli")
        .define("BUILD_SHARED_LIBS", "OFF")
        .define("GGML_NATIVE", "OFF")
        // Use ggml's own thread pool; the packaged worker has no OpenMP DLL/dylib.
        .define("GGML_OPENMP", "OFF")
        .define("GGML_METAL", if metal { "ON" } else { "OFF" })
        .define("GGML_METAL_EMBED_LIBRARY", if metal { "ON" } else { "OFF" })
        .define("GGML_ACCELERATE", if macos { "ON" } else { "OFF" })
        .define("GGML_VULKAN", "OFF")
        .define("GGML_CUDA", "OFF");
    if macos {
        config.define("CMAKE_OSX_DEPLOYMENT_TARGET", "12.0");
    }
    if target.contains("windows-msvc") {
        config
            .static_crt(true)
            .define("CMAKE_POLICY_DEFAULT_CMP0091", "NEW")
            .define("CMAKE_MSVC_RUNTIME_LIBRARY", "MultiThreaded");
    }
    let built = config.build().join("build");
    let name = if target.contains("windows") {
        "speech.exe"
    } else {
        "speech"
    };
    let binary = [built.join(name), built.join("Release").join(name)]
        .into_iter()
        .find(|path| path.is_file())
        .ok_or("speech.cpp build did not produce its worker executable")?;
    // OUT_DIR = <target>/<profile>/build/<crate-hash>/out, also for cross builds.
    let profile = out
        .ancestors()
        .nth(3)
        .ok_or("unexpected Cargo output layout")?;
    let packaged_name = if target.contains("windows") {
        "saymore-speech.exe"
    } else {
        "saymore-speech"
    };
    fs::copy(binary, profile.join(packaged_name))?;
    Ok(())
}

fn unpack_source(
    out: &Path,
    repository: &str,
    revision: &str,
    sha256: &str,
) -> Result<PathBuf, Box<dyn Error>> {
    let destination = out.join(format!("{repository}-{revision}"));
    if destination.join(".saymore-verified").is_file() {
        return Ok(destination);
    }
    let archive = out.join(format!("{repository}-{revision}.tar.gz"));
    if !archive.is_file() {
        let url = format!("https://codeload.github.com/nyosegawa/{repository}/tar.gz/{revision}");
        let response = ureq::get(&url).timeout(Duration::from_secs(120)).call()?;
        let mut reader = response.into_reader();
        let temporary = archive.with_extension("download");
        io::copy(&mut reader, &mut fs::File::create(&temporary)?)?;
        fs::rename(temporary, &archive)?;
    }
    let bytes = fs::read(&archive)?;
    if format!("{:x}", Sha256::digest(&bytes)) != sha256 {
        fs::remove_file(&archive)?;
        return Err(format!("{repository} source archive failed SHA-256 validation").into());
    }
    if destination.exists() {
        fs::remove_dir_all(&destination)?;
    }
    tar::Archive::new(flate2::read::GzDecoder::new(bytes.as_slice())).unpack(out)?;
    if !destination.join("CMakeLists.txt").is_file() {
        return Err(format!("{repository} source archive has an unexpected layout").into());
    }
    fs::write(destination.join(".saymore-verified"), sha256)?;
    Ok(destination)
}
