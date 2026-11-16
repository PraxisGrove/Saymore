//! Build the pinned, isolated native Qwen worker. It is packaged beside the app.
use sha2::{Digest, Sha256};
use std::{
    env,
    error::Error,
    fs, io,
    path::{Path, PathBuf},
    time::Duration,
};

const SPEECH_REVISION: &str = "c4bdcd0e751bd29460cb1353e6d54499828a1d90";
const GGML_REVISION: &str = "cbdeccfd49ec9c702c175c273d36c47520cf4710";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if let Err(error) = build_worker() {
        eprintln!("could not build the pinned Qwen speech.cpp worker: {error}");
        std::process::exit(1);
    }
}

fn build_worker() -> Result<(), Box<dyn Error>> {
    let out = PathBuf::from(env::var_os("OUT_DIR").ok_or("missing OUT_DIR")?);
    let source = unpack_source(
        &out,
        "speech.cpp",
        SPEECH_REVISION,
        "cf233c7b44eec7b1a91c299fd7ad5b67930ec46e1e913f970fd86bc2ea13cafa",
    )?;
    let ggml = unpack_source(
        &out,
        "ggml",
        GGML_REVISION,
        "03f751738259e0c0a8a6a98b15934afee8e6768aa21c815f75eb4a5a2ad40b50",
    )?;
    // Source archives do not contain the submodule. Copy only the pinned tree.
    let submodule = source.join("ggml");
    if !submodule.join("CMakeLists.txt").is_file() {
        copy_tree(&ggml, &submodule)?;
    }
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

fn copy_tree(source: &Path, destination: &Path) -> io::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}
