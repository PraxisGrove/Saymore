use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const APP_NAME: &str = "Saymore";
const BINARY_NAME: &str = "saymore-desktop";
const BUNDLE_IDENTIFIER: &str = "com.saymore.desktop";

pub(crate) fn run() -> Result<(), Box<dyn Error>> {
    let root = workspace_root()?;
    build_release(&root)?;

    let app = root.join("target/release/bundle/macos/Saymore.app");
    if app.exists() {
        fs::remove_dir_all(&app)?;
    }

    let contents = app.join("Contents");
    let macos = contents.join("MacOS");
    let resources = contents.join("Resources");
    fs::create_dir_all(&macos)?;
    fs::create_dir_all(&resources)?;

    fs::copy(
        root.join("target/release").join(BINARY_NAME),
        macos.join(BINARY_NAME),
    )?;
    fs::copy(
        root.join("apps/desktop/icons/icon.icns"),
        resources.join("icon.icns"),
    )?;
    fs::write(contents.join("Info.plist"), info_plist())?;
    sign(&app)?;

    println!("[INFO] bundled {}", app.display());
    Ok(())
}

fn workspace_root() -> Result<PathBuf, Box<dyn Error>> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    Ok(manifest_dir
        .parent()
        .and_then(Path::parent)
        .ok_or("xtask must live under <workspace>/crates/xtask")?
        .to_owned())
}

fn build_release(root: &Path) -> Result<(), Box<dyn Error>> {
    let status = Command::new("cargo")
        .args(["build", "-p", BINARY_NAME, "--release"])
        .current_dir(root)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err("release build failed".into())
    }
}

fn sign(app: &Path) -> Result<(), Box<dyn Error>> {
    let requirement = format!("=designated => identifier \"{BUNDLE_IDENTIFIER}\"");
    let status = Command::new("codesign")
        .args([
            "--force",
            "--deep",
            "--sign",
            "-",
            "--identifier",
            BUNDLE_IDENTIFIER,
            "--requirements",
            &requirement,
        ])
        .arg(app)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err("ad-hoc code signing failed".into())
    }
}

fn info_plist() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDisplayName</key><string>{APP_NAME}</string>
  <key>CFBundleExecutable</key><string>{BINARY_NAME}</string>
  <key>CFBundleIconFile</key><string>icon.icns</string>
  <key>CFBundleIdentifier</key><string>{BUNDLE_IDENTIFIER}</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>{APP_NAME}</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSMicrophoneUsageDescription</key><string>Saymore 使用麦克风将语音转换为文字。</string>
</dict>
</plist>
"#
    )
}
