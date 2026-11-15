//! macOS delivery probe restricted to a dedicated TextEdit benchmark document.
#[cfg(target_os = "macos")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::{io, process::Command, time::Instant};
    use template_app::{AccessibilityAuthorization, TextDeliverer};
    use template_infra::MacOsTextDeliverer;
    let deliverer = MacOsTextDeliverer;
    if deliverer.authorization() != AccessibilityAuthorization::Granted {
        return Err(io::Error::other("accessibility authorization unavailable").into());
    }
    let checked = Command::new("osascript")
        .args([
            "-e",
            r#"tell application "TextEdit"
if not frontmost then return "blocked"
if (count documents) is 0 then return "blocked"
if name of front document is not "Saymore Delivery Benchmark" then return "blocked"
return "ready"
end tell"#,
        ])
        .output()?;
    if !checked.status.success() || String::from_utf8_lossy(&checked.stdout).trim() != "ready" {
        return Err(
            io::Error::other("focus the dedicated Saymore Delivery Benchmark document").into(),
        );
    }
    let started = Instant::now();
    let outcome = deliverer.deliver("Saymore delivery benchmark completed.")?;
    println!(
        "{}",
        serde_json::json!({"delivery_seconds":started.elapsed().as_secs_f64(),
        "outcome":format!("{outcome:?}"),"scope":"native delivery only; excludes capture, ASR and refinement"})
    );
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("macos_text_delivery_probe requires macOS");
}
