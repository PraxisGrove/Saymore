use std::{
    env,
    error::Error,
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

pub(crate) const PREVIEW_SIGNING_IDENTITY: &str = "Saymore Preview Development";
pub(crate) const PREVIEW_KEYCHAIN_FILENAME: &str = "preview-signing.keychain-db";

const KEYCHAIN_PASSWORD_FILENAME: &str = "keychain-password";
const KEYCHAIN_UNLOCK_SECONDS: &str = "21600";

// TCC treats every rebuilt ad-hoc binary as new code, so Preview keeps one
// local certificate and keychain across rebuilds.
pub(crate) fn ensure_preview_signing() -> Result<PathBuf, Box<dyn Error>> {
    let home = env::var_os("HOME").ok_or("HOME is unavailable for Preview signing")?;
    let directory =
        PathBuf::from(home).join("Library/Application Support/Saymore Dev/preview-signing");
    ensure_directory_permissions(&directory)?;

    let keychain = directory.join(PREVIEW_KEYCHAIN_FILENAME);
    let password_file = directory.join(KEYCHAIN_PASSWORD_FILENAME);
    match (keychain.exists(), password_file.exists()) {
        (false, false) => create_preview_identity(&directory, &keychain, &password_file)?,
        (true, true) => {}
        _ => {
            return Err(format!(
                "Preview signing state is incomplete under {}; remove that directory and run Preview again",
                directory.display()
            )
            .into());
        }
    }

    let password = read_password(&password_file)?;
    unlock_keychain(&keychain, &password)?;
    ensure_keychain_search_list(&keychain)?;
    verify_identity(&keychain)?;
    Ok(directory)
}

fn ensure_directory_permissions(directory: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(directory)?;
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn create_preview_identity(
    directory: &Path,
    keychain: &Path,
    password_file: &Path,
) -> Result<(), Box<dyn Error>> {
    let password = generate_password()?;
    let artifacts = SigningArtifacts::create()?;
    generate_certificate(&artifacts)?;
    create_pkcs12(&artifacts, &password)?;

    run(
        Command::new("/usr/bin/security")
            .args(["create-keychain", "-p", &password])
            .arg(keychain),
        "create the Preview signing keychain",
    )?;
    let provisioned = provision_keychain(
        keychain,
        &artifacts.pkcs12,
        &artifacts.certificate,
        &password,
    );
    if let Err(error) = provisioned {
        let _ = Command::new("/usr/bin/security")
            .arg("delete-keychain")
            .arg(keychain)
            .status();
        return Err(error);
    }

    write_password(password_file, &password)?;
    println!(
        "[PREVIEW] created persistent local signing identity in {}",
        directory.display()
    );
    println!("[PREVIEW] re-enable Accessibility once after this signing migration");
    Ok(())
}

fn generate_password() -> Result<String, Box<dyn Error>> {
    let output = output(
        Command::new("/usr/bin/openssl").args(["rand", "-hex", "32"]),
        "generate the Preview keychain password",
    )?;
    let password = String::from_utf8(output.stdout)?.trim().to_owned();
    if password.is_empty() {
        Err("openssl returned an empty Preview keychain password".into())
    } else {
        Ok(password)
    }
}

fn generate_certificate(artifacts: &SigningArtifacts) -> Result<(), Box<dyn Error>> {
    run(
        Command::new("/usr/bin/openssl")
            .args([
                "req",
                "-new",
                "-newkey",
                "rsa:2048",
                "-x509",
                "-sha256",
                "-days",
                "3650",
                "-nodes",
                "-subj",
                "/CN=Saymore Preview Development/O=Saymore Local Development",
                "-addext",
                "keyUsage=critical,digitalSignature",
                "-addext",
                "extendedKeyUsage=codeSigning",
                "-keyout",
            ])
            .arg(&artifacts.private_key)
            .arg("-out")
            .arg(&artifacts.certificate),
        "generate the Preview signing certificate",
    )
}

fn create_pkcs12(artifacts: &SigningArtifacts, password: &str) -> Result<(), Box<dyn Error>> {
    let passout = format!("pass:{password}");
    run(
        Command::new("/usr/bin/openssl")
            .args(["pkcs12", "-export", "-descert", "-inkey"])
            .arg(&artifacts.private_key)
            .arg("-in")
            .arg(&artifacts.certificate)
            .arg("-out")
            .arg(&artifacts.pkcs12)
            .args(["-passout", &passout, "-name", PREVIEW_SIGNING_IDENTITY]),
        "package the Preview signing identity",
    )
}

fn provision_keychain(
    keychain: &Path,
    pkcs12: &Path,
    certificate: &Path,
    password: &str,
) -> Result<(), Box<dyn Error>> {
    run(
        Command::new("/usr/bin/security")
            .args(["set-keychain-settings", "-lut", KEYCHAIN_UNLOCK_SECONDS])
            .arg(keychain),
        "configure the Preview signing keychain",
    )?;
    unlock_keychain(keychain, password)?;
    run(
        Command::new("/usr/bin/security")
            .arg("import")
            .arg(pkcs12)
            .arg("-k")
            .arg(keychain)
            .args(["-P", password, "-T", "/usr/bin/codesign"]),
        "import the Preview signing identity",
    )?;
    run(
        Command::new("/usr/bin/security")
            .args([
                "set-key-partition-list",
                "-S",
                "apple-tool:,apple:,codesign:",
                "-s",
                "-k",
                password,
            ])
            .arg(keychain),
        "authorize codesign to use the Preview signing identity",
    )?;
    println!("[PREVIEW] macOS will ask once to trust the local code-signing certificate");
    run(
        Command::new("/usr/bin/security")
            .args(["add-trusted-cert", "-r", "trustRoot", "-p", "codeSign"])
            .arg("-k")
            .arg(keychain)
            .arg(certificate),
        "trust the Preview code-signing certificate",
    )?;
    ensure_keychain_search_list(keychain)?;
    verify_identity(keychain)
}

fn unlock_keychain(keychain: &Path, password: &str) -> Result<(), Box<dyn Error>> {
    run(
        Command::new("/usr/bin/security")
            .args(["unlock-keychain", "-p", password])
            .arg(keychain),
        "unlock the Preview signing keychain",
    )
}

fn ensure_keychain_search_list(keychain: &Path) -> Result<(), Box<dyn Error>> {
    let result = output(
        Command::new("/usr/bin/security").args(["list-keychains", "-d", "user"]),
        "inspect the user keychain search list",
    )?;
    let mut keychains = parse_keychain_search_list(&result.stdout);
    if keychains.iter().any(|candidate| candidate == keychain) {
        return Ok(());
    }

    keychains.push(keychain.to_owned());
    let mut command = Command::new("/usr/bin/security");
    command.args(["list-keychains", "-d", "user", "-s"]);
    command.args(&keychains);
    run(
        &mut command,
        "add the Preview keychain to the user search list",
    )
}

fn parse_keychain_search_list(output: &[u8]) -> Vec<PathBuf> {
    String::from_utf8_lossy(output)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| PathBuf::from(line.trim_matches('"')))
        .collect()
}

fn verify_identity(keychain: &Path) -> Result<(), Box<dyn Error>> {
    let result = output(
        Command::new("/usr/bin/security")
            .args(["find-identity", "-v", "-p", "codesigning"])
            .arg(keychain),
        "inspect the Preview signing identity",
    )?;
    let identities = String::from_utf8_lossy(&result.stdout);
    if identities.contains(PREVIEW_SIGNING_IDENTITY) {
        Ok(())
    } else {
        Err(format!(
            "{PREVIEW_SIGNING_IDENTITY} is unavailable in {}",
            keychain.display()
        )
        .into())
    }
}

fn write_password(path: &Path, password: &str) -> Result<(), Box<dyn Error>> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(password.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(())
}

fn read_password(path: &Path) -> Result<String, Box<dyn Error>> {
    let password = fs::read_to_string(path)?.trim().to_owned();
    if password.is_empty() {
        Err(format!("Preview keychain password is empty at {}", path.display()).into())
    } else {
        Ok(password)
    }
}

fn run(command: &mut Command, operation: &str) -> Result<(), Box<dyn Error>> {
    let result = command.output()?;
    if result.status.success() {
        Ok(())
    } else {
        Err(command_error(operation, &result).into())
    }
}

fn output(command: &mut Command, operation: &str) -> Result<Output, Box<dyn Error>> {
    let result = command.output()?;
    if result.status.success() {
        Ok(result)
    } else {
        Err(command_error(operation, &result).into())
    }
}

fn command_error(operation: &str, result: &Output) -> String {
    let stderr = String::from_utf8_lossy(&result.stderr);
    let detail = stderr.trim();
    if detail.is_empty() {
        format!("failed to {operation}: {}", result.status)
    } else {
        format!("failed to {operation}: {detail}")
    }
}

struct SigningArtifacts {
    directory: PathBuf,
    private_key: PathBuf,
    certificate: PathBuf,
    pkcs12: PathBuf,
}

impl SigningArtifacts {
    fn create() -> Result<Self, Box<dyn Error>> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let directory = env::temp_dir().join(format!(
            "saymore-preview-signing-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&directory)?;
        Ok(Self {
            private_key: directory.join("private-key.pem"),
            certificate: directory.join("certificate.pem"),
            pkcs12: directory.join("identity.p12"),
            directory,
        })
    }
}

impl Drop for SigningArtifacts {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_keychain_search_list_without_quotes_or_empty_lines() {
        assert_eq!(
            vec![
                PathBuf::from("/Users/example/Library/Keychains/login.keychain-db"),
                PathBuf::from("/Users/example/Library/Keychains/preview.keychain-db"),
            ],
            parse_keychain_search_list(
                br#"
                    "/Users/example/Library/Keychains/login.keychain-db"
                    "/Users/example/Library/Keychains/preview.keychain-db"
                "#,
            )
        );
    }
}
