use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub(crate) const BINARY_NAME: &str = "saymore-desktop";
const DEVELOPMENT_MARKER: &str = "saymore-development-environment";
const MICROPHONE_USAGE_DESCRIPTION_EN: &str =
    "Saymore uses the microphone to transcribe your speech.";
const MICROPHONE_USAGE_DESCRIPTION_ZH_HANS: &str = "Saymore 使用麦克风将你的语音转写为文字。";
const ENTITLEMENTS_PATH: &str = "apps/desktop/packaging/macos/entitlements.plist";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BundleSigning {
    AdHoc,
    Keychain { identity: String, keychain: PathBuf },
}

pub(crate) struct BundleSpec<'a> {
    pub app_name: &'a str,
    pub bundle_identifier: &'a str,
    pub development_environment: bool,
    pub signing: BundleSigning,
}

pub(crate) fn run() -> Result<(), Box<dyn Error>> {
    let root = workspace_root()?;
    build(&root, BuildProfile::Release)?;
    let app = root.join("target/release/bundle/macos/Saymore.app");
    create_bundle(
        &root,
        &root.join("target/release").join(BINARY_NAME),
        &app,
        BundleSpec {
            app_name: "Saymore",
            bundle_identifier: "com.saymore.desktop",
            development_environment: false,
            signing: BundleSigning::AdHoc,
        },
    )?;
    println!("[INFO] bundled {}", app.display());
    Ok(())
}

pub(crate) fn build_debug(root: &Path) -> Result<(), Box<dyn Error>> {
    build(root, BuildProfile::Debug)
}

pub(crate) fn create_bundle(
    root: &Path,
    source_binary: &Path,
    app: &Path,
    spec: BundleSpec<'_>,
) -> Result<(), Box<dyn Error>> {
    if app.exists() {
        fs::remove_dir_all(app)?;
    }

    let contents = app.join("Contents");
    let macos = contents.join("MacOS");
    let resources = contents.join("Resources");
    fs::create_dir_all(&macos)?;
    fs::create_dir_all(&resources)?;

    fs::copy(source_binary, macos.join(BINARY_NAME))?;
    fs::copy(
        root.join("apps/desktop/icons/icon.icns"),
        resources.join("icon.icns"),
    )?;
    if spec.development_environment {
        fs::write(resources.join(DEVELOPMENT_MARKER), [])?;
    }
    write_localized_info_plist_strings(&resources)?;
    fs::write(contents.join("Info.plist"), info_plist(&spec))?;
    sign(
        app,
        spec.bundle_identifier,
        &root.join(ENTITLEMENTS_PATH),
        &spec.signing,
    )?;
    Ok(())
}

fn write_localized_info_plist_strings(resources: &Path) -> Result<(), std::io::Error> {
    for (locale, description) in [
        ("en", MICROPHONE_USAGE_DESCRIPTION_EN),
        ("zh-Hans", MICROPHONE_USAGE_DESCRIPTION_ZH_HANS),
    ] {
        let localization = resources.join(format!("{locale}.lproj"));
        fs::create_dir_all(&localization)?;
        fs::write(
            localization.join("InfoPlist.strings"),
            format!("\"NSMicrophoneUsageDescription\" = \"{description}\";\n"),
        )?;
    }
    Ok(())
}

pub(crate) fn workspace_root() -> Result<PathBuf, Box<dyn Error>> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    Ok(manifest_dir
        .parent()
        .and_then(Path::parent)
        .ok_or("xtask must live under <workspace>/crates/xtask")?
        .to_owned())
}

#[derive(Clone, Copy)]
enum BuildProfile {
    Debug,
    Release,
}

fn build(root: &Path, profile: BuildProfile) -> Result<(), Box<dyn Error>> {
    let mut command = Command::new("cargo");
    command.args(["build", "-p", BINARY_NAME]);
    if matches!(profile, BuildProfile::Release) {
        command.arg("--release");
    }
    let status = command.current_dir(root).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(match profile {
            BuildProfile::Debug => "debug build failed",
            BuildProfile::Release => "release build failed",
        }
        .into())
    }
}

fn sign(
    app: &Path,
    bundle_identifier: &str,
    entitlements: &Path,
    signing: &BundleSigning,
) -> Result<(), Box<dyn Error>> {
    let mut command = Command::new("codesign");
    command
        .args(["--force", "--deep", "--identifier", bundle_identifier])
        .arg("--entitlements")
        .arg(entitlements);
    match signing {
        BundleSigning::AdHoc => {
            let requirement = format!("=designated => identifier \"{bundle_identifier}\"");
            command.args(["--sign", "-", "--requirements", &requirement]);
        }
        BundleSigning::Keychain { identity, keychain } => {
            command
                .args(["--sign", identity, "--keychain"])
                .arg(keychain);
        }
    }
    let status = command.arg(app).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(match signing {
            BundleSigning::AdHoc => "ad-hoc code signing failed",
            BundleSigning::Keychain { .. } => "keychain code signing failed",
        }
        .into())
    }
}

fn info_plist(spec: &BundleSpec<'_>) -> String {
    let app_name = spec.app_name;
    let bundle_identifier = spec.bundle_identifier;
    let version = env!("CARGO_PKG_VERSION");
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDisplayName</key><string>{app_name}</string>
  <key>CFBundleDevelopmentRegion</key><string>en</string>
  <key>CFBundleExecutable</key><string>{BINARY_NAME}</string>
  <key>CFBundleIconFile</key><string>icon.icns</string>
  <key>CFBundleIdentifier</key><string>{bundle_identifier}</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleLocalizations</key>
  <array><string>en</string><string>zh-Hans</string></array>
  <key>CFBundleName</key><string>{app_name}</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>{version}</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSMicrophoneUsageDescription</key><string>{MICROPHONE_USAGE_DESCRIPTION_EN}</string>
</dict>
</plist>
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_plist_keeps_a_stable_distinct_identity() {
        let plist = info_plist(&BundleSpec {
            app_name: "Saymore Preview",
            bundle_identifier: "com.saymore.desktop.preview",
            development_environment: true,
            signing: BundleSigning::AdHoc,
        });

        assert!(plist.contains("<string>Saymore Preview</string>"));
        assert!(plist.contains("<string>com.saymore.desktop.preview</string>"));
        assert!(plist.contains(MICROPHONE_USAGE_DESCRIPTION_EN));
        assert!(plist.contains("<key>CFBundleDevelopmentRegion</key><string>en</string>"));
        assert!(plist.contains("<string>zh-Hans</string>"));
    }

    #[test]
    fn writes_localized_microphone_usage_descriptions() -> Result<(), Box<dyn std::error::Error>> {
        let resources = std::env::temp_dir().join(format!(
            "saymore-xtask-localizations-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&resources);
        write_localized_info_plist_strings(&resources)?;

        let english = fs::read_to_string(resources.join("en.lproj/InfoPlist.strings"))?;
        let simplified_chinese =
            fs::read_to_string(resources.join("zh-Hans.lproj/InfoPlist.strings"))?;
        let expected_english =
            format!("\"NSMicrophoneUsageDescription\" = \"{MICROPHONE_USAGE_DESCRIPTION_EN}\";\n");
        let expected_simplified_chinese = format!(
            "\"NSMicrophoneUsageDescription\" = \"{MICROPHONE_USAGE_DESCRIPTION_ZH_HANS}\";\n"
        );
        if english != expected_english {
            return Err("English microphone usage description should match".into());
        }
        if simplified_chinese != expected_simplified_chinese {
            return Err("Simplified Chinese microphone usage description should match".into());
        }
        let _ = fs::remove_dir_all(resources);
        Ok(())
    }
}
