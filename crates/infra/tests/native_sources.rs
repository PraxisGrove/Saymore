#![allow(clippy::panic_in_result_fn)]
use std::{fs, path::Path};

#[path = "../build/native_sources.rs"]
mod native_sources;

fn source_at(root: &Path) -> std::io::Result<()> {
    fs::create_dir_all(root.join("src"))?;
    fs::write(root.join("CMakeLists.txt"), "add_subdirectory(src)")?;
    fs::write(root.join("src").join("engine.cpp"), "build input")?;
    fs::write(root.join("LICENSE"), "upstream license")
}

#[test]
fn copies_build_inputs_and_licenses_without_agent_metadata()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("source");
    let target = directory.path().join("target");
    source_at(&source)?;
    let metadata = source.join(".pi");
    fs::create_dir_all(metadata.join("gg"))?;
    // Match the archive's POSIX target. Leave it dangling to exercise unreadable
    // non-build metadata on Unix as well as the Windows path failure.
    #[cfg(unix)]
    std::os::unix::fs::symlink("gg/SYSTEM.md", metadata.join("SYSTEM.md"))?;
    #[cfg(windows)]
    std::os::windows::fs::symlink_file("gg/SYSTEM.md", metadata.join("SYSTEM.md"))?;
    native_sources::stage(&source, &target, "revision")?;
    assert_eq!(
        fs::read_to_string(target.join("src").join("engine.cpp"))?,
        "build input"
    );
    assert_eq!(
        fs::read_to_string(target.join("LICENSE"))?,
        "upstream license"
    );
    assert!(!target.join(".pi").exists());
    Ok(())
}

#[test]
fn rebuilds_an_incomplete_destination_instead_of_trusting_its_cmake_file()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("source");
    let target = directory.path().join("target");
    source_at(&source)?;
    fs::create_dir_all(&target)?;
    fs::write(target.join("CMakeLists.txt"), "partial copy")?;
    fs::write(target.join("stale.cpp"), "must not survive")?;
    native_sources::stage(&source, &target, "revision")?;
    assert_eq!(
        fs::read_to_string(target.join("src").join("engine.cpp"))?,
        "build input"
    );
    assert!(!target.join("stale.cpp").exists());
    Ok(())
}
