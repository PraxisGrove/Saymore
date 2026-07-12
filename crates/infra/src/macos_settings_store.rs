use std::{
    env, fs,
    fs::{File, OpenOptions, Permissions},
    io::{BufReader, BufWriter, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process,
};

use serde::{Deserialize, Serialize};
use template_app::{
    AsrSettings, SaymoreSettings, SettingsStore, SettingsStoreError, VolcengineAsrSettings,
};

const CONFIG_VERSION: u32 = 1;

pub struct JsonSettingsStore {
    path: PathBuf,
}

impl JsonSettingsStore {
    pub fn for_current_user() -> Result<Self, SettingsStoreError> {
        let home = env::var_os("HOME")
            .ok_or_else(|| SettingsStoreError::Unavailable("HOME is not defined".to_owned()))?;
        Ok(Self {
            path: PathBuf::from(home).join("Library/Application Support/Saymore/config.json"),
        })
    }

    #[cfg(test)]
    fn at_path(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn ensure_exists(&self) -> Result<(), SettingsStoreError> {
        if self.path.exists() {
            Ok(())
        } else {
            self.save(&SaymoreSettings::default())
        }
    }
}

impl SettingsStore for JsonSettingsStore {
    fn load(&self) -> Result<SaymoreSettings, SettingsStoreError> {
        if !self.path.exists() {
            return Ok(SaymoreSettings::default());
        }
        let file = File::open(&self.path).map_err(io_error)?;
        let stored: StoredSettings =
            serde_json::from_reader(BufReader::new(file)).map_err(json_error)?;
        stored.try_into()
    }

    fn save(&self, settings: &SaymoreSettings) -> Result<(), SettingsStoreError> {
        let parent = self.path.parent().ok_or_else(|| {
            SettingsStoreError::Unavailable("settings path has no parent".to_owned())
        })?;
        fs::create_dir_all(parent).map_err(io_error)?;
        fs::set_permissions(parent, Permissions::from_mode(0o700)).map_err(io_error)?;

        let temporary = temporary_path(&self.path);
        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(io_error)?;
        let mut writer = BufWriter::new(file);
        serde_json::to_writer_pretty(&mut writer, &StoredSettings::from(settings))
            .map_err(json_error)?;
        writer.write_all(b"\n").map_err(io_error)?;
        writer.flush().map_err(io_error)?;
        writer.get_ref().sync_all().map_err(io_error)?;
        fs::rename(&temporary, &self.path).map_err(io_error)?;
        fs::set_permissions(&self.path, Permissions::from_mode(0o600)).map_err(io_error)?;
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredSettings {
    version: u32,
    #[serde(default)]
    asr: StoredAsrSettings,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct StoredAsrSettings {
    #[serde(default)]
    volcengine: StoredVolcengineAsrSettings,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct StoredVolcengineAsrSettings {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    auth_mode: StoredAuthMode,
    #[serde(default)]
    api_key: String,
    #[serde(default)]
    model: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StoredAuthMode {
    #[default]
    ApiKey,
}

impl From<&SaymoreSettings> for StoredSettings {
    fn from(settings: &SaymoreSettings) -> Self {
        Self {
            version: CONFIG_VERSION,
            asr: StoredAsrSettings {
                volcengine: StoredVolcengineAsrSettings {
                    enabled: settings.asr.volcengine.enabled,
                    auth_mode: StoredAuthMode::ApiKey,
                    api_key: settings.asr.volcengine.api_key.clone(),
                    model: settings.asr.volcengine.model.clone(),
                },
            },
        }
    }
}

impl TryFrom<StoredSettings> for SaymoreSettings {
    type Error = SettingsStoreError;

    fn try_from(stored: StoredSettings) -> Result<Self, Self::Error> {
        if stored.version != CONFIG_VERSION {
            return Err(SettingsStoreError::Invalid(format!(
                "unsupported config version {}",
                stored.version
            )));
        }
        Ok(Self {
            asr: AsrSettings {
                volcengine: VolcengineAsrSettings {
                    enabled: stored.asr.volcengine.enabled,
                    api_key: stored.asr.volcengine.api_key,
                    model: stored.asr.volcengine.model,
                },
            },
        })
    }
}

fn temporary_path(path: &Path) -> PathBuf {
    path.with_extension(format!("json.tmp-{}", process::id()))
}

fn io_error(error: std::io::Error) -> SettingsStoreError {
    SettingsStoreError::Unavailable(error.to_string())
}

fn json_error(error: serde_json::Error) -> SettingsStoreError {
    SettingsStoreError::Invalid(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    static TEST_ID: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn saves_and_loads_volcengine_settings_with_private_permissions() {
        let directory = test_directory();
        let path = directory.join("config.json");
        let store = JsonSettingsStore::at_path(path.clone());
        let settings = SaymoreSettings {
            asr: AsrSettings {
                volcengine: VolcengineAsrSettings {
                    enabled: true,
                    api_key: "test-key".to_owned(),
                    model: "test-model".to_owned(),
                },
            },
        };

        assert!(store.save(&settings).is_ok());
        assert_eq!(Ok(settings), store.load());
        let Ok(metadata) = fs::metadata(&path) else {
            panic!("saved settings should have metadata");
        };
        assert_eq!(0o600, metadata.permissions().mode() & 0o777);

        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn rejects_unknown_config_versions() {
        let directory = test_directory();
        let path = directory.join("config.json");
        assert!(fs::create_dir_all(&directory).is_ok());
        assert!(fs::write(&path, r#"{"version":99,"asr":{}}"#).is_ok());
        let store = JsonSettingsStore::at_path(path);

        assert!(matches!(store.load(), Err(SettingsStoreError::Invalid(_))));

        let _ = fs::remove_dir_all(directory);
    }

    fn test_directory() -> PathBuf {
        let id = TEST_ID.fetch_add(1, Ordering::Relaxed);
        env::temp_dir().join(format!("saymore-settings-{}-{id}", process::id()))
    }
}
