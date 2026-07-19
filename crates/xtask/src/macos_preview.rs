use std::{
    collections::BTreeMap,
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, SystemTime},
};

use crate::macos_bundle::{
    BINARY_NAME, BundleSigning, BundleSpec, build_debug, create_bundle, workspace_root,
};
use crate::macos_preview_signing::{
    PREVIEW_KEYCHAIN_FILENAME, PREVIEW_SIGNING_IDENTITY, ensure_preview_signing,
};

const PREVIEW_APP: &str = "/Applications/Saymore Preview.app";
const PREVIEW_BUNDLE_IDENTIFIER: &str = "com.saymore.desktop.preview";
const POLL_INTERVAL: Duration = Duration::from_millis(500);
const CHANGE_DEBOUNCE: Duration = Duration::from_millis(250);
const SHUTDOWN_RETRIES: usize = 40;
const HELPER_TERMINATION_RETRIES: usize = 4;
const PROCESS_CHECK_INTERVAL: Duration = Duration::from_millis(50);
const PREVIEW_RUNNING_PATTERN: &str =
    r"^/Applications/Saymore Preview\.app/Contents/MacOS/saymore-desktop( |$)";
const PREVIEW_HELPER_NAMES: [&str; 2] = [
    "AutoFill (Saymore Preview)",
    "ThemeWidgetControlViewService (Saymore Preview)",
];

pub(crate) fn run(args: &[String]) -> Result<(), Box<dyn Error>> {
    let once = parse_once(args)?;
    let root = workspace_root()?;
    refresh(&root)?;
    print_permission_hint();
    if once {
        return Ok(());
    }

    println!("[PREVIEW] watching for Rust, Slint, Cargo, and desktop asset changes");
    let mut snapshot = SourceSnapshot::capture_app(&root)?;
    let tooling_snapshot = SourceSnapshot::capture_tooling(&root)?;
    loop {
        thread::sleep(POLL_INTERVAL);
        if SourceSnapshot::capture_tooling(&root)? != tooling_snapshot {
            println!("[PREVIEW] preview tooling changed; restarting the watcher");
            return Ok(());
        }

        let next = SourceSnapshot::capture_app(&root)?;
        if next == snapshot {
            continue;
        }

        thread::sleep(CHANGE_DEBOUNCE);
        snapshot = SourceSnapshot::capture_app(&root)?;
        println!("\n[PREVIEW] change detected; rebuilding debug app");
        if let Err(error) = refresh(&root) {
            eprintln!("[PREVIEW] rebuild failed; keeping the current preview open: {error}");
        }
    }
}

fn parse_once(args: &[String]) -> Result<bool, Box<dyn Error>> {
    match args {
        [] => Ok(false),
        [option] if option == "--once" => Ok(true),
        [option] => Err(format!("unknown preview-macos option: {option}").into()),
        _ => Err("usage: cargo run -p xtask -- preview-macos [--once]".into()),
    }
}

fn refresh(root: &Path) -> Result<(), Box<dyn Error>> {
    build_debug(root)?;
    let signing_directory = ensure_preview_signing()?;
    let staged_app = root.join("target/debug/bundle/macos/Saymore Preview.app");
    create_bundle(
        root,
        &root.join("target/debug").join(BINARY_NAME),
        &staged_app,
        preview_bundle_spec(&signing_directory),
    )?;

    stop_running_apps()?;
    install(&staged_app)?;
    launch()?;
    println!("[PREVIEW] running {PREVIEW_APP}");
    Ok(())
}

fn preview_bundle_spec(signing_directory: &Path) -> BundleSpec<'static> {
    BundleSpec {
        app_name: "Saymore Preview",
        bundle_identifier: PREVIEW_BUNDLE_IDENTIFIER,
        development_environment: true,
        signing: BundleSigning::Keychain {
            identity: PREVIEW_SIGNING_IDENTITY.to_owned(),
            keychain: signing_directory.join(PREVIEW_KEYCHAIN_FILENAME),
        },
    }
}

fn stop_running_apps() -> Result<(), Box<dyn Error>> {
    let status = Command::new("/usr/bin/pkill")
        .args(["-TERM", "-f", PREVIEW_RUNNING_PATTERN])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if !status.success() && status.code() != Some(1) {
        return Err("failed to stop Saymore Preview".into());
    }

    for _ in 0..SHUTDOWN_RETRIES {
        if !any_saymore_process_running()? {
            return stop_preview_helpers();
        }
        thread::sleep(PROCESS_CHECK_INTERVAL);
    }
    Err("Saymore did not stop before the preview update".into())
}

fn stop_preview_helpers() -> Result<(), Box<dyn Error>> {
    signal_preview_helpers("-TERM")?;
    if wait_for_preview_helpers_to_stop(HELPER_TERMINATION_RETRIES)? {
        return Ok(());
    }

    signal_preview_helpers("-KILL")?;
    if wait_for_preview_helpers_to_stop(SHUTDOWN_RETRIES)? {
        Ok(())
    } else {
        Err("Saymore Preview helpers did not stop before the preview update".into())
    }
}

fn signal_preview_helpers(signal: &str) -> Result<(), Box<dyn Error>> {
    for pid in preview_helper_pids()? {
        let _ = Command::new("/bin/kill")
            .args([signal, &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
    }
    Ok(())
}

fn wait_for_preview_helpers_to_stop(retries: usize) -> Result<bool, Box<dyn Error>> {
    for _ in 0..retries {
        if preview_helper_pids()?.is_empty() {
            return Ok(true);
        }
        thread::sleep(PROCESS_CHECK_INTERVAL);
    }
    Ok(preview_helper_pids()?.is_empty())
}

fn preview_helper_pids() -> Result<Vec<u32>, Box<dyn Error>> {
    let output = Command::new("/usr/bin/lsappinfo").arg("list").output()?;
    if !output.status.success() {
        return Err("failed to inspect Preview helpers".into());
    }
    Ok(parse_preview_helper_pids(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

fn parse_preview_helper_pids(output: &str) -> Vec<u32> {
    let mut matching_entry = false;
    let mut pids = Vec::new();
    for line in output.lines() {
        if !line.starts_with(char::is_whitespace) && line.contains(") \"") {
            matching_entry = PREVIEW_HELPER_NAMES
                .iter()
                .any(|name| line.contains(&format!("\"{name}\"")));
            continue;
        }
        if !matching_entry {
            continue;
        }
        let Some(pid) = line.trim().strip_prefix("pid = ") else {
            continue;
        };
        if let Some(pid) = pid
            .split_whitespace()
            .next()
            .and_then(|pid| pid.parse().ok())
        {
            pids.push(pid);
        }
    }
    pids
}

fn any_saymore_process_running() -> Result<bool, Box<dyn Error>> {
    let status = Command::new("/usr/bin/pgrep")
        .args(["-f", PREVIEW_RUNNING_PATTERN])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if status.success() {
        return Ok(true);
    }
    if status.code() == Some(1) {
        Ok(false)
    } else {
        Err("failed to inspect Saymore Preview process".into())
    }
}

fn install(staged_app: &Path) -> Result<(), Box<dyn Error>> {
    let status = Command::new("/usr/bin/ditto")
        .args(["--rsrc", "--extattr", "--acl"])
        .arg(staged_app)
        .arg(PREVIEW_APP)
        .status()?;
    if !status.success() {
        return Err("failed to install Saymore Preview.app".into());
    }

    let status = Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(PREVIEW_APP)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err("installed Saymore Preview.app failed code-sign verification".into())
    }
}

fn launch() -> Result<(), Box<dyn Error>> {
    let status = Command::new("/usr/bin/open").arg(PREVIEW_APP).status()?;
    if !status.success() {
        return Err("failed to launch Saymore Preview.app".into());
    }

    for _ in 0..SHUTDOWN_RETRIES {
        let status = Command::new("/usr/bin/pgrep")
            .args(["-f", PREVIEW_RUNNING_PATTERN])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        if status.success() {
            return Ok(());
        }
        if status.code() != Some(1) {
            return Err("failed to verify Saymore Preview.app launch".into());
        }
        thread::sleep(PROCESS_CHECK_INTERVAL);
    }
    Err("Saymore Preview.app did not start".into())
}

fn print_permission_hint() {
    println!(
        "[PREVIEW] first use: enable Saymore Preview in System Settings > Privacy & Security > Accessibility"
    );
    println!("[PREVIEW] the preview keeps this identity across debug rebuilds");
    println!(
        "[PREVIEW] Preview and production data are isolated, but both currently listen for Right Command"
    );
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceSnapshot(BTreeMap<PathBuf, FileStamp>);

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileStamp {
    modified: Option<SystemTime>,
    length: u64,
}

impl SourceSnapshot {
    fn capture_app(root: &Path) -> Result<Self, Box<dyn Error>> {
        let mut files = Vec::new();
        collect_watch_files(&root.join("apps/desktop"), &mut files)?;
        for crate_name in ["app", "domain", "infra"] {
            collect_watch_files(&root.join("crates").join(crate_name), &mut files)?;
        }
        Self::from_files(files)
    }

    fn capture_tooling(root: &Path) -> Result<Self, Box<dyn Error>> {
        let mut files = Vec::new();
        collect_watch_files(&root.join("crates/xtask"), &mut files)?;
        for name in ["Cargo.toml", "Cargo.lock"] {
            let path = root.join(name);
            if path.is_file() {
                files.push(path);
            }
        }
        let preview_script = root.join("scripts/dev-preview.sh");
        if preview_script.is_file() {
            files.push(preview_script);
        }
        Self::from_files(files)
    }

    fn from_files(mut files: Vec<PathBuf>) -> Result<Self, Box<dyn Error>> {
        files.sort();
        files.dedup();

        let mut stamps = BTreeMap::new();
        for path in files {
            let metadata = fs::metadata(&path)?;
            stamps.insert(
                path,
                FileStamp {
                    modified: metadata.modified().ok(),
                    length: metadata.len(),
                },
            );
        }
        Ok(Self(stamps))
    }
}

fn collect_watch_files(directory: &Path, files: &mut Vec<PathBuf>) -> Result<(), Box<dyn Error>> {
    if !directory.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_watch_files(&path, files)?;
        } else if is_watch_file(&path) {
            files.push(path);
        }
    }
    Ok(())
}

fn is_watch_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some(
            "rs" | "slint"
                | "toml"
                | "lock"
                | "json"
                | "svg"
                | "png"
                | "icns"
                | "ttf"
                | "otf"
                | "wav"
                | "mp3"
        )
    )
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn parses_watch_and_single_refresh_modes() {
        assert_eq!(
            Ok(false),
            parse_once(&[]).map_err(|error| error.to_string())
        );
        assert_eq!(
            Ok(true),
            parse_once(&["--once".to_owned()]).map_err(|error| error.to_string())
        );
        assert!(parse_once(&["--unknown".to_owned()]).is_err());
    }

    #[test]
    fn preview_bundle_uses_a_stable_bundle_and_signing_identity() {
        let signing_directory = Path::new("preview-signing");
        let spec = preview_bundle_spec(signing_directory);

        assert_eq!("Saymore Preview", spec.app_name);
        assert_eq!(PREVIEW_BUNDLE_IDENTIFIER, spec.bundle_identifier);
        assert_eq!(
            BundleSigning::Keychain {
                identity: PREVIEW_SIGNING_IDENTITY.to_owned(),
                keychain: signing_directory.join(PREVIEW_KEYCHAIN_FILENAME),
            },
            spec.signing
        );
    }

    #[test]
    fn parses_only_preview_helpers() {
        let output = r#"1) "AutoFill (Safari)" ASN:0x0-0x1:
    pid = 11 type="BackgroundOnly"
2) "AutoFill (Saymore Preview)" ASN:0x0-0x2:
    bundleID="com.apple.SafariPlatformSupport.Helper"
    pid = 22 type="BackgroundOnly"
3) "ThemeWidgetControlViewService (Safari)" ASN:0x0-0x3:
    pid = 33 type="BackgroundOnly"
4) "ThemeWidgetControlViewService (Saymore Preview)" ASN:0x0-0x4:
    pid = 44 type="Foreground"
5) "AutoFill (Saymore Preview)" ASN:0x0-0x5:
    pid = 55 type="BackgroundOnly"
"#;

        assert_eq!(vec![22, 44, 55], parse_preview_helper_pids(output));
    }

    #[test]
    fn snapshot_changes_when_a_watched_file_changes() -> Result<(), Box<dyn Error>> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let root = std::env::temp_dir().join(format!("saymore-preview-{nonce}"));
        let source = root.join("apps/desktop/ui/preview.slint");
        let parent = source.parent().ok_or("temporary source has no parent")?;
        fs::create_dir_all(parent)?;
        fs::create_dir_all(root.join("crates"))?;
        fs::write(&source, "export component Preview {}")?;
        let before = SourceSnapshot::capture_app(&root)?;

        fs::write(&source, "export component Preview { width: 1px; }")?;
        let after = SourceSnapshot::capture_app(&root)?;
        fs::remove_dir_all(root)?;

        if before == after {
            Err("preview snapshot did not detect the source change".into())
        } else {
            Ok(())
        }
    }

    #[test]
    fn snapshot_changes_when_preview_tooling_changes() -> Result<(), Box<dyn Error>> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let root = std::env::temp_dir().join(format!("saymore-preview-tooling-{nonce}"));
        let source = root.join("crates/xtask/src/macos_preview.rs");
        let parent = source.parent().ok_or("temporary source has no parent")?;
        fs::create_dir_all(parent)?;
        fs::write(&source, "fn preview() {}")?;
        let before = SourceSnapshot::capture_tooling(&root)?;

        fs::write(&source, "fn preview() { println!(\"changed\"); }")?;
        let after = SourceSnapshot::capture_tooling(&root)?;
        fs::remove_dir_all(root)?;

        if before == after {
            Err("preview tooling snapshot did not detect the source change".into())
        } else {
            Ok(())
        }
    }
}
