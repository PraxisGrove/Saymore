#[cfg(target_os = "windows")]
use std::{env, error::Error, io};

#[cfg(target_os = "windows")]
use template_infra::copy_text_to_clipboard;

#[cfg(target_os = "windows")]
fn main() -> Result<(), Box<dyn Error>> {
    let mut arguments = env::args().skip(1);
    let text = arguments.next().ok_or_else(usage)?;
    if arguments.next().is_some() || text.is_empty() {
        return Err(usage().into());
    }
    copy_text_to_clipboard(&text)?;
    println!("PASS: copied {} UTF-8 bytes", text.len());
    Ok(())
}

#[cfg(target_os = "windows")]
fn usage() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "usage: windows_clipboard_probe TEXT",
    )
}

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("windows_clipboard_probe is available only on Windows");
}
