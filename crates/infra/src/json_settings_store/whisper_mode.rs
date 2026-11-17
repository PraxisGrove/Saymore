use super::JsonSettingsStore;
use template_app::{SettingsStoreError, WhisperRecognitionMode};

impl JsonSettingsStore {
    /// Changes only Whisper's mode under the catalog lock; it never selects a provider.
    pub fn set_whisper_recognition_mode(
        &self,
        mode: WhisperRecognitionMode,
    ) -> Result<(), SettingsStoreError> {
        let _access = self.lock_access()?;
        let mut catalog = self.load_catalog_unlocked()?;
        catalog.set_whisper_recognition_mode(mode)?;
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
        catalog.select_qwen3_asr_provider();
        catalog.set_qwen_recognition_mode(template_app::QwenRecognitionMode::Streaming)?;
        store.save_catalog(&catalog)?;
        store.set_whisper_recognition_mode(WhisperRecognitionMode::Streaming)?;
        let loaded = JsonSettingsStore::at_path(path).load_catalog()?;
        assert!(loaded.qwen3_asr_is_active());
        assert_eq!(
            loaded.qwen_recognition_mode()?,
            template_app::QwenRecognitionMode::Streaming
        );
        assert_eq!(
            WhisperRecognitionMode::Streaming,
            loaded.whisper_recognition_mode()?
        );
        store.set_whisper_recognition_mode(WhisperRecognitionMode::NonStreaming)?;
        assert_eq!(
            WhisperRecognitionMode::NonStreaming,
            store.load_catalog()?.whisper_recognition_mode()?
        );
        Ok(())
    }
}
