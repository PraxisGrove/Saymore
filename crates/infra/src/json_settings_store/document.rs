use std::{
    collections::BTreeMap,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::Path,
};

use template_app::{ProviderCatalog, SettingsStoreError};

use super::schema::{LegacySettings, StoredCatalog, legacy_catalog};
use super::{
    io_error, json_error, platform_fs, retired_asr_is_active, temporary_path, validate_catalog,
};

pub(super) fn read(path: &Path) -> Result<Option<serde_json::Value>, SettingsStoreError> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_error(error)),
    };
    serde_json::from_reader(BufReader::new(file))
        .map(Some)
        .map_err(json_error)
}

pub(super) fn normalize(
    value: &mut serde_json::Value,
    import_connections: bool,
) -> Result<(ProviderCatalog, bool), SettingsStoreError> {
    let version = value
        .get("version")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| SettingsStoreError::Invalid("config version is missing".to_owned()))?;
    match version {
        1 | 2 if import_connections => {
            let legacy: LegacySettings =
                serde_json::from_value(value.clone()).map_err(json_error)?;
            let catalog = legacy_catalog(legacy);
            *value = serde_json::to_value(StoredCatalog::from(&catalog)).map_err(json_error)?;
            Ok((catalog, true))
        }
        1 | 2 => {
            let legacy: LegacySettings =
                serde_json::from_value(value.clone()).map_err(json_error)?;
            Ok((legacy_catalog(legacy), false))
        }
        3 => {
            let stored: StoredCatalog =
                serde_json::from_value(value.clone()).map_err(json_error)?;
            let (mut catalog, mut shared) = stored.into_catalog();
            let magpie_imported = if import_connections {
                super::magpie::import(&mut catalog, &mut shared)?
            } else {
                false
            };
            let retired_asr_selected = retired_asr_is_active(&catalog);
            if retired_asr_selected {
                catalog.active.asr = None;
            }
            let changed = magpie_imported || retired_asr_selected;
            if changed {
                *value = serde_json::to_value(StoredCatalog::new(&catalog, shared))
                    .map_err(json_error)?;
            }
            validate_catalog(&catalog)?;
            Ok((catalog, changed))
        }
        other => Err(SettingsStoreError::Invalid(format!(
            "unsupported config version {other}"
        ))),
    }
}

pub(super) fn shared_fields(
    document: serde_json::Value,
) -> Result<BTreeMap<String, serde_json::Value>, SettingsStoreError> {
    let mut root = document.as_object().cloned().ok_or_else(|| {
        SettingsStoreError::Invalid("provider configuration must be an object".to_owned())
    })?;
    for field in ["version", "active", "asr_providers", "llm_providers"] {
        root.remove(field);
    }
    Ok(root.into_iter().collect())
}

pub(super) fn save(path: &Path, document: &serde_json::Value) -> Result<(), SettingsStoreError> {
    let stored: StoredCatalog = serde_json::from_value(document.clone()).map_err(json_error)?;
    let (catalog, _) = stored.into_catalog();
    validate_catalog(&catalog)?;
    let parent = path
        .parent()
        .ok_or_else(|| SettingsStoreError::Unavailable("settings path has no parent".to_owned()))?;
    std::fs::create_dir_all(parent).map_err(io_error)?;
    platform_fs::restrict_directory_permissions(parent)?;

    let temporary = temporary_path(path);
    let result = (|| {
        let file = platform_fs::open_private_file(&temporary)?;
        let mut writer = BufWriter::new(file);
        serde_json::to_writer_pretty(&mut writer, document).map_err(json_error)?;
        writer.write_all(b"\n").map_err(io_error)?;
        writer.flush().map_err(io_error)?;
        writer.get_ref().sync_all().map_err(io_error)?;
        drop(writer);
        platform_fs::atomic_replace(&temporary, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}
