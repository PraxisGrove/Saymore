use std::{ffi::OsStr, io, process::Command};

pub(crate) fn open(target: impl AsRef<OsStr>) -> Result<(), io::Error> {
    command(target.as_ref()).spawn().map(|_| ())
}

#[cfg(target_os = "macos")]
fn command(target: &OsStr) -> Command {
    let mut command = Command::new("/usr/bin/open");
    command.arg(target);
    command
}

#[cfg(target_os = "windows")]
fn command(target: &OsStr) -> Command {
    let mut command = Command::new("explorer.exe");
    command.arg(target);
    command
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn command(target: &OsStr) -> Command {
    let mut command = Command::new("xdg-open");
    command.arg(target);
    command
}
