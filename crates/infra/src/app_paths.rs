use std::{env, path::PathBuf};

use template_app::StorageError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppEnvironment {
    Production,
    Development,
}

impl AppEnvironment {
    fn data_directory_name(self) -> &'static str {
        match self {
            Self::Production => "Saymore",
            Self::Development => "Saymore Dev",
        }
    }

    pub(crate) fn history_secret_service(self) -> &'static str {
        match self {
            Self::Production => "com.saymore.desktop",
            Self::Development => "com.saymore.desktop.dev",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    data_directory: PathBuf,
}

impl AppPaths {
    pub fn for_current_user(environment: AppEnvironment) -> Result<Self, StorageError> {
        #[cfg(target_os = "macos")]
        let data_directory = env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join("Library/Application Support"));
        #[cfg(target_os = "windows")]
        let data_directory = env::var_os("LOCALAPPDATA").map(PathBuf::from);
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let data_directory = env::var_os("XDG_DATA_HOME").map(PathBuf::from);
        data_directory
            .map(|root| Self {
                data_directory: root.join(environment.data_directory_name()),
            })
            .ok_or_else(|| {
                StorageError::Unavailable(
                    "the current user's application data directory is unavailable".to_owned(),
                )
            })
    }

    pub fn data_directory(&self) -> &std::path::Path {
        &self.data_directory
    }

    pub fn database(&self) -> PathBuf {
        self.data_directory.join("saymore.sqlite3")
    }

    pub fn provider_config(&self) -> PathBuf {
        self.data_directory.join("config.json")
    }

    pub fn instance_lock(&self) -> PathBuf {
        self.data_directory.join("saymore.instance.lock")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environments_use_distinct_data_directories_and_secret_services() {
        let root = PathBuf::from("application-data");
        let production = root.join(AppEnvironment::Production.data_directory_name());
        let development = root.join(AppEnvironment::Development.data_directory_name());

        assert_eq!(PathBuf::from("application-data/Saymore"), production);
        assert_eq!(PathBuf::from("application-data/Saymore Dev"), development);
        assert_ne!(production, development);
        assert_ne!(
            AppEnvironment::Production.history_secret_service(),
            AppEnvironment::Development.history_secret_service()
        );
    }
}
