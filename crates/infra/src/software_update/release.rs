use super::{DOWNLOAD_PREFIX, UpdatePlatform};
use serde::Deserialize;
use template_app::{UpdateError, UpdatePackage, is_newer_release, release_version};

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    draft: bool,
    assets: Vec<Asset>,
}
#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    size: u64,
}

pub(super) fn select(
    bytes: &[u8],
    current: &str,
    platform: UpdatePlatform,
) -> Result<Option<UpdatePackage>, UpdateError> {
    let release: Release = serde_json::from_slice(bytes).map_err(|_| UpdateError::Metadata)?;
    if release.prerelease || release.draft || !is_newer_release(&release.tag_name, current) {
        return Ok(None);
    }
    let installers = release
        .assets
        .iter()
        .filter(|asset| match platform {
            UpdatePlatform::MacOs => asset.name.ends_with(".dmg"),
            UpdatePlatform::Windows => asset.name == "Saymore-Setup.exe",
            UpdatePlatform::Unsupported => false,
        })
        .collect::<Vec<_>>();
    let [asset] = installers.as_slice() else {
        return Err(UpdateError::Unsupported);
    };
    let checksums = release
        .assets
        .iter()
        .find(|asset| asset.name == "SHA256SUMS")
        .ok_or(UpdateError::Metadata)?;
    let package = UpdatePackage {
        version: release.tag_name,
        file_name: asset.name.clone(),
        download_url: asset.browser_download_url.clone(),
        checksums_url: checksums.browser_download_url.clone(),
        size: asset.size,
    };
    validate(&package)?;
    Ok(Some(package))
}

pub(super) fn validate(package: &UpdatePackage) -> Result<(), UpdateError> {
    let name = &package.file_name;
    if release_version(&package.version).is_none()
        || package.size == 0
        || name.is_empty()
        || name.starts_with('.')
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    {
        return Err(UpdateError::Metadata);
    }
    let prefix = format!("{DOWNLOAD_PREFIX}{}/", package.version);
    if package.download_url != format!("{prefix}{name}")
        || package.checksums_url != format!("{prefix}SHA256SUMS")
    {
        return Err(UpdateError::Metadata);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[allow(clippy::panic_in_result_fn)]
    fn selects_platform_installer_and_rejects_foreign_urls()
    -> Result<(), Box<dyn std::error::Error>> {
        let release = serde_json::json!({"tag_name":"v1.0.0", "assets":[
            {"name":"Saymore.dmg", "size":10, "browser_download_url":format!("{DOWNLOAD_PREFIX}v1.0.0/Saymore.dmg")},
            {"name":"Saymore-Setup.exe", "size":20, "browser_download_url":format!("{DOWNLOAD_PREFIX}v1.0.0/Saymore-Setup.exe")},
            {"name":"SHA256SUMS", "size":100, "browser_download_url":format!("{DOWNLOAD_PREFIX}v1.0.0/SHA256SUMS")}
        ]});
        let bytes = serde_json::to_vec(&release)?;
        let mut package = select(&bytes, "0.2.10", UpdatePlatform::Windows)?
            .ok_or("missing Windows installer")?;
        assert_eq!(package.file_name, "Saymore-Setup.exe");
        let mac_package =
            select(&bytes, "0.2.10", UpdatePlatform::MacOs)?.ok_or("missing macOS installer")?;
        assert_eq!(mac_package.file_name, "Saymore.dmg");
        package.download_url = "https://example.com/Saymore-Setup.exe".into();
        assert!(validate(&package).is_err());
        assert!(select(&bytes, "1.0.0", UpdatePlatform::Windows)?.is_none());
        Ok(())
    }
}
