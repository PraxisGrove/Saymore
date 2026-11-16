use super::JsonSettingsStore;
use template_app::{QwenRecognitionMode, SettingsStoreError};

impl JsonSettingsStore {
    /// Changes only Qwen's mode under the catalog lock; it never selects a provider.
    pub fn set_qwen_recognition_mode(
        &self,
        mode: QwenRecognitionMode,
    ) -> Result<(), SettingsStoreError> {
        let _access = self.lock_access()?;
        let mut catalog = self.load_catalog_unlocked()?;
        catalog.set_qwen_recognition_mode(mode)?;
        self.save_catalog_unlocked(&catalog)
    }
}

#[cfg(test)]
#[allow(clippy::panic_in_result_fn)]
mod tests {
    use super::*;
    use template_app::{ProviderCatalog, ProviderConfigStore};

    #[test]
    fn mode_round_trips_without_changing_the_active_provider()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("settings.json");
        let store = JsonSettingsStore::at_path(path.clone());
        let mut catalog = ProviderCatalog::default();
        catalog.select_whisper_provider();
        store.save_catalog(&catalog)?;
        store.set_qwen_recognition_mode(QwenRecognitionMode::Streaming)?;
        let loaded = JsonSettingsStore::at_path(path).load_catalog()?;
        assert!(loaded.whisper_is_active());
        assert_eq!(
            QwenRecognitionMode::Streaming,
            loaded.qwen_recognition_mode()?
        );
        store.set_qwen_recognition_mode(QwenRecognitionMode::NonStreaming)?;
        assert_eq!(
            QwenRecognitionMode::NonStreaming,
            store.load_catalog()?.qwen_recognition_mode()?
        );
        Ok(())
    }
}
