#[cfg(target_os = "macos")]
use std::{
    fs::{self, OpenOptions, Permissions},
    io::{self, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

use keyring::{Entry, Error as KeyringError};
use template_app::{SecretStore, SecretStoreError};

use crate::app_paths::AppEnvironment;
#[cfg(target_os = "macos")]
use crate::app_paths::AppPaths;

const HISTORY_KEY_ACCOUNT: &str = "local-history-data-key-v2";
const LEGACY_HISTORY_KEY_ACCOUNT: &str = "local-history-data-key-v1";

#[derive(Debug)]
pub struct PlatformSecretStore {
    backend: SecretBackend,
}

#[derive(Debug)]
enum SecretBackend {
    Keychain {
        service: String,
    },
    #[cfg(target_os = "macos")]
    File(FileSecretStore),
}

#[cfg(target_os = "macos")]
#[derive(Debug)]
struct FileSecretStore {
    path: PathBuf,
}

impl PlatformSecretStore {
    pub fn new(environment: AppEnvironment) -> Result<Self, SecretStoreError> {
        #[cfg(target_os = "macos")]
        if environment == AppEnvironment::Development {
            let paths = AppPaths::for_current_user(environment)
                .map_err(|error| SecretStoreError::Unavailable(error.to_string()))?;
            return Ok(Self {
                backend: SecretBackend::File(FileSecretStore::new(paths.development_history_key())),
            });
        }

        Ok(Self {
            backend: SecretBackend::Keychain {
                service: environment.history_secret_service().to_owned(),
            },
        })
    }

    fn entry(service: &str, account: &str) -> Result<Entry, SecretStoreError> {
        Entry::new(service, account).map_err(map_error)
    }

    fn load_keychain_secret(
        service: &str,
        account: &str,
    ) -> Result<Option<Vec<u8>>, SecretStoreError> {
        match Self::entry(service, account)?.get_secret() {
            Ok(secret) => Ok(Some(secret)),
            Err(KeyringError::NoEntry) => Ok(None),
            Err(error) => Err(map_error(error)),
        }
    }

    fn save_keychain_secret(
        service: &str,
        account: &str,
        secret: &[u8],
    ) -> Result<(), SecretStoreError> {
        Self::entry(service, account)?
            .set_secret(secret)
            .map_err(map_error)
    }

    fn delete_keychain_secret(service: &str, account: &str) -> Result<(), SecretStoreError> {
        match Self::entry(service, account)?.delete_credential() {
            Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
            Err(error) => Err(map_error(error)),
        }
    }
}

impl SecretStore for PlatformSecretStore {
    fn load_history_key(&self) -> Result<Option<Vec<u8>>, SecretStoreError> {
        match &self.backend {
            SecretBackend::Keychain { service } => load_or_migrate_history_key(
                |account| Self::load_keychain_secret(service, account),
                |account, secret| Self::save_keychain_secret(service, account, secret),
            ),
            #[cfg(target_os = "macos")]
            SecretBackend::File(file) => file.load_or_migrate(|| {
                let service = AppEnvironment::Development.history_secret_service();
                load_or_migrate_history_key(
                    |account| Self::load_keychain_secret(service, account),
                    |account, secret| Self::save_keychain_secret(service, account, secret),
                )
            }),
        }
    }

    fn save_history_key(&self, key: &[u8]) -> Result<(), SecretStoreError> {
        match &self.backend {
            SecretBackend::Keychain { service } => {
                Self::save_keychain_secret(service, HISTORY_KEY_ACCOUNT, key)
            }
            #[cfg(target_os = "macos")]
            SecretBackend::File(file) => file.save(key),
        }
    }

    fn delete_history_key(&self) -> Result<(), SecretStoreError> {
        match &self.backend {
            SecretBackend::Keychain { service } => {
                Self::delete_keychain_secret(service, HISTORY_KEY_ACCOUNT)?;
                Self::delete_keychain_secret(service, LEGACY_HISTORY_KEY_ACCOUNT)
            }
            #[cfg(target_os = "macos")]
            SecretBackend::File(file) => file.delete(),
        }
    }
}

#[cfg(target_os = "macos")]
impl FileSecretStore {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn load_or_migrate(
        &self,
        migrate: impl FnOnce() -> Result<Option<Vec<u8>>, SecretStoreError>,
    ) -> Result<Option<Vec<u8>>, SecretStoreError> {
        if let Some(key) = self.load()? {
            return Ok(Some(key));
        }
        let Some(key) = migrate()? else {
            return Ok(None);
        };
        self.save(&key)?;
        Ok(Some(key))
    }

    fn load(&self) -> Result<Option<Vec<u8>>, SecretStoreError> {
        match fs::read(&self.path) {
            Ok(secret) => Ok(Some(secret)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(file_error(&self.path, error)),
        }
    }

    fn save(&self, secret: &[u8]) -> Result<(), SecretStoreError> {
        let parent = self.path.parent().ok_or_else(|| {
            SecretStoreError::Invalid("history key path has no parent".to_owned())
        })?;
        fs::create_dir_all(parent).map_err(|error| file_error(parent, error))?;
        fs::set_permissions(parent, Permissions::from_mode(0o700))
            .map_err(|error| file_error(parent, error))?;

        let temporary = temporary_path(&self.path);
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(|error| file_error(&temporary, error))?;
        file.set_permissions(Permissions::from_mode(0o600))
            .map_err(|error| file_error(&temporary, error))?;
        file.write_all(secret)
            .map_err(|error| file_error(&temporary, error))?;
        file.sync_all()
            .map_err(|error| file_error(&temporary, error))?;
        fs::rename(&temporary, &self.path).map_err(|error| file_error(&self.path, error))
    }

    fn delete(&self) -> Result<(), SecretStoreError> {
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(file_error(&self.path, error)),
        }
    }
}

#[cfg(target_os = "macos")]
fn temporary_path(path: &Path) -> PathBuf {
    path.with_extension(format!("tmp-{}", std::process::id()))
}

#[cfg(target_os = "macos")]
fn file_error(path: &Path, error: io::Error) -> SecretStoreError {
    SecretStoreError::Unavailable(format!("{}: {error}", path.display()))
}

fn load_or_migrate_history_key(
    mut load: impl FnMut(&str) -> Result<Option<Vec<u8>>, SecretStoreError>,
    mut save: impl FnMut(&str, &[u8]) -> Result<(), SecretStoreError>,
) -> Result<Option<Vec<u8>>, SecretStoreError> {
    if let Some(key) = load(HISTORY_KEY_ACCOUNT)? {
        return Ok(Some(key));
    }
    let Some(key) = load(LEGACY_HISTORY_KEY_ACCOUNT)? else {
        return Ok(None);
    };
    save(HISTORY_KEY_ACCOUNT, &key)?;
    Ok(Some(key))
}

fn map_error(error: KeyringError) -> SecretStoreError {
    match error {
        KeyringError::TooLong(_, _)
        | KeyringError::Invalid(_, _)
        | KeyringError::BadEncoding(_)
        | KeyringError::Ambiguous(_) => SecretStoreError::Invalid(
            "the operating-system credential store rejected the credential".to_owned(),
        ),
        KeyringError::PlatformFailure(_)
        | KeyringError::NoStorageAccess(_)
        | KeyringError::NoEntry => SecretStoreError::Unavailable(
            "the operating-system credential store is unavailable".to_owned(),
        ),
        _ => SecretStoreError::Unavailable(
            "the operating-system credential store operation failed".to_owned(),
        ),
    }
}

#[cfg(all(test, target_os = "windows"))]
mod windows_tests {
    use super::*;

    struct TestCredential {
        service: String,
    }

    impl TestCredential {
        fn new() -> Self {
            Self {
                service: format!(
                    "com.saymore.desktop.test.{}.{}",
                    std::process::id(),
                    uuid::Uuid::new_v4()
                ),
            }
        }

        fn store(&self) -> PlatformSecretStore {
            PlatformSecretStore {
                backend: SecretBackend::Keychain {
                    service: self.service.clone(),
                },
            }
        }
    }

    impl Drop for TestCredential {
        fn drop(&mut self) {
            let _ = PlatformSecretStore::delete_keychain_secret(&self.service, HISTORY_KEY_ACCOUNT);
            let _ = PlatformSecretStore::delete_keychain_secret(
                &self.service,
                LEGACY_HISTORY_KEY_ACCOUNT,
            );
        }
    }

    #[test]
    fn credential_manager_round_trips_overwrites_deletes_and_migrates_legacy_key() {
        let credential = TestCredential::new();
        let store = credential.store();
        let first = vec![1, 3, 5, 7];
        let replacement = vec![2, 4, 6, 8];

        assert_eq!(Ok(None), store.load_history_key());
        assert_eq!(Ok(()), store.save_history_key(&first));
        assert_eq!(Ok(Some(first)), store.load_history_key());
        assert_eq!(Ok(()), store.save_history_key(&replacement));
        assert_eq!(Ok(Some(replacement)), store.load_history_key());
        assert_eq!(Ok(()), store.delete_history_key());
        assert_eq!(Ok(None), store.load_history_key());

        let legacy = vec![9, 7, 5, 3];
        assert_eq!(
            Ok(()),
            PlatformSecretStore::save_keychain_secret(
                &credential.service,
                LEGACY_HISTORY_KEY_ACCOUNT,
                &legacy,
            )
        );
        assert_eq!(Ok(Some(legacy.clone())), store.load_history_key());
        assert_eq!(
            Ok(Some(legacy)),
            PlatformSecretStore::load_keychain_secret(&credential.service, HISTORY_KEY_ACCOUNT,)
        );
        assert_eq!(Ok(()), store.delete_history_key());
        assert_eq!(Ok(None), store.load_history_key());
    }

    #[test]
    fn production_and_development_use_stable_distinct_services() {
        assert_eq!(
            "com.saymore.desktop",
            AppEnvironment::Production.history_secret_service()
        );
        assert_eq!(
            "com.saymore.desktop.dev",
            AppEnvironment::Development.history_secret_service()
        );
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use std::{cell::RefCell, collections::BTreeMap, os::unix::fs::PermissionsExt};

    use super::*;
    use tempfile::tempdir;

    #[test]
    fn legacy_history_key_is_copied_to_the_current_entry() {
        let legacy_key = vec![1, 2, 3, 4];
        let entries = RefCell::new(BTreeMap::from([(
            LEGACY_HISTORY_KEY_ACCOUNT.to_owned(),
            legacy_key.clone(),
        )]));

        let loaded = load_or_migrate_history_key(
            |account| Ok(entries.borrow().get(account).cloned()),
            |account, key| {
                entries
                    .borrow_mut()
                    .insert(account.to_owned(), key.to_vec());
                Ok(())
            },
        );

        assert_eq!(Ok(Some(legacy_key.clone())), loaded);
        assert_eq!(
            BTreeMap::from([
                (HISTORY_KEY_ACCOUNT.to_owned(), legacy_key.clone()),
                (LEGACY_HISTORY_KEY_ACCOUNT.to_owned(), legacy_key),
            ]),
            entries.into_inner()
        );
    }

    #[test]
    fn current_history_key_does_not_access_the_legacy_entry() {
        let current_key = vec![4, 3, 2, 1];
        let loaded_accounts = RefCell::new(Vec::new());
        let saved_entries = RefCell::new(Vec::new());

        let loaded = load_or_migrate_history_key(
            |account| {
                loaded_accounts.borrow_mut().push(account.to_owned());
                Ok((account == HISTORY_KEY_ACCOUNT).then(|| current_key.clone()))
            },
            |account, key| {
                saved_entries
                    .borrow_mut()
                    .push((account.to_owned(), key.to_vec()));
                Ok(())
            },
        );

        assert_eq!(Ok(Some(current_key)), loaded);
        assert_eq!(
            vec![HISTORY_KEY_ACCOUNT.to_owned()],
            loaded_accounts.into_inner()
        );
        assert_eq!(Vec::<(String, Vec<u8>)>::new(), saved_entries.into_inner());
    }

    #[test]
    fn file_secret_store_round_trips_with_private_permissions() {
        let Ok(directory) = tempdir() else {
            panic!("temporary directory should be available");
        };
        let parent = directory.path().join("private");
        let path = parent.join("history-data-key");
        let store = FileSecretStore::new(path.clone());
        let key = vec![9, 8, 7, 6];

        assert_eq!(Ok(()), store.save(&key));
        let Ok(directory_metadata) = fs::metadata(parent) else {
            panic!("private directory should exist");
        };
        let Ok(key_metadata) = fs::metadata(path) else {
            panic!("history key should exist");
        };

        assert_eq!(Ok(Some(key)), store.load());
        assert_eq!(0o700, directory_metadata.permissions().mode() & 0o777);
        assert_eq!(0o600, key_metadata.permissions().mode() & 0o777);
    }

    #[test]
    fn existing_file_secret_does_not_access_the_migration_source() {
        let Ok(directory) = tempdir() else {
            panic!("temporary directory should be available");
        };
        let store = FileSecretStore::new(directory.path().join("history-data-key"));
        let key = vec![5, 6, 7, 8];
        assert_eq!(Ok(()), store.save(&key));
        let migration_attempted = RefCell::new(false);

        let loaded = store.load_or_migrate(|| {
            *migration_attempted.borrow_mut() = true;
            Ok(Some(vec![1, 2, 3, 4]))
        });

        assert_eq!(Ok(Some(key)), loaded);
        assert!(!migration_attempted.into_inner());
    }
}
