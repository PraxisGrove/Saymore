use keyring::{Entry, Error as KeyringError};
use template_app::{SecretStore, SecretStoreError};

use crate::app_paths::AppEnvironment;

const HISTORY_KEY_ACCOUNT: &str = "local-history-data-key-v1";

#[derive(Debug)]
pub struct PlatformSecretStore {
    service: &'static str,
}

impl PlatformSecretStore {
    pub fn new(environment: AppEnvironment) -> Self {
        Self {
            service: environment.history_secret_service(),
        }
    }

    fn entry(&self) -> Result<Entry, SecretStoreError> {
        Entry::new(self.service, HISTORY_KEY_ACCOUNT).map_err(map_error)
    }
}

impl SecretStore for PlatformSecretStore {
    fn load_history_key(&self) -> Result<Option<Vec<u8>>, SecretStoreError> {
        match self.entry()?.get_secret() {
            Ok(secret) => Ok(Some(secret)),
            Err(KeyringError::NoEntry) => Ok(None),
            Err(error) => Err(map_error(error)),
        }
    }

    fn save_history_key(&self, key: &[u8]) -> Result<(), SecretStoreError> {
        self.entry()?.set_secret(key).map_err(map_error)
    }

    fn delete_history_key(&self) -> Result<(), SecretStoreError> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
            Err(error) => Err(map_error(error)),
        }
    }
}

fn map_error(error: KeyringError) -> SecretStoreError {
    match error {
        KeyringError::TooLong(_, _)
        | KeyringError::Invalid(_, _)
        | KeyringError::BadEncoding(_)
        | KeyringError::Ambiguous(_) => SecretStoreError::Invalid(error.to_string()),
        KeyringError::PlatformFailure(_)
        | KeyringError::NoStorageAccess(_)
        | KeyringError::NoEntry => SecretStoreError::Unavailable(error.to_string()),
        _ => SecretStoreError::Unavailable(error.to_string()),
    }
}
