use std::{
    env,
    error::Error,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use template_infra::{ModelInstallControl, VerifiedModelInstaller};

pub fn install_model(
    label: &str,
    installer: &VerifiedModelInstaller,
    report_interval_bytes: u64,
) -> Result<PathBuf, Box<dyn Error>> {
    if env::var_os("SAYMORE_PROBE_REMOVE").is_some() {
        installer.remove()?;
        println!("Removed the isolated {label} installation.");
    }
    println!("Installed before install: {}", installer.is_installed());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let control = ModelInstallControl::default();
    let progress_control = control.clone();
    let pause_at = env::var("SAYMORE_PROBE_PAUSE_AT_BYTES")
        .ok()
        .and_then(|value| value.parse::<u64>().ok());
    let last_reported = Arc::new(AtomicU64::new(0));
    let progress_reported = Arc::clone(&last_reported);
    runtime
        .block_on(installer.install_with_control(
            Arc::new(move |progress| {
                let previous = progress_reported.load(Ordering::Relaxed);
                if progress.downloaded_bytes == progress.total_bytes
                    || progress.downloaded_bytes.saturating_sub(previous) >= report_interval_bytes
                {
                    progress_reported.store(progress.downloaded_bytes, Ordering::Relaxed);
                    println!(
                        "Download: {}/{} bytes",
                        progress.downloaded_bytes, progress.total_bytes
                    );
                }
                if pause_at.is_some_and(|threshold| progress.downloaded_bytes >= threshold) {
                    progress_control.pause();
                }
            }),
            control,
        ))
        .map_err(Into::into)
}
