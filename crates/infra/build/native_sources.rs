//! Stage a complete copy of verified ggml build inputs.
use std::{fs, io, path::Path};

pub(super) fn stage(source: &Path, destination: &Path, revision: &str) -> io::Result<()> {
    let marker = destination.join(".saymore-copy-complete");
    if destination.join("CMakeLists.txt").is_file()
        && fs::read_to_string(&marker).is_ok_and(|cached| cached == revision)
    {
        return Ok(());
    }
    if destination.exists() {
        fs::remove_dir_all(destination)?;
    }
    copy_tree(source, destination)?;
    // A partial copy must never be reused after an I/O error.
    fs::write(marker, revision)
}

fn copy_tree(source: &Path, destination: &Path) -> io::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        // Upstream agent metadata is not a build input. Its SYSTEM.md symlink
        // has a POSIX target that Windows cannot follow when copying files.
        if entry.file_name() == ".pi" {
            continue;
        }
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target).map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!(
                        "copy native source {} to {}: {error}",
                        entry.path().display(),
                        target.display()
                    ),
                )
            })?;
        }
    }
    Ok(())
}
