use super::{
    ProviderCatalog, ProviderInstance, SettingsStoreError, WHISPER_PROVIDER_ID,
    WHISPER_PROVIDER_TYPE,
};
use serde::{Deserialize, Serialize};

/// Selects Whisper's repeated-window live transcription or existing offline segments.
/// A dictation snapshots this choice at start; changing it never changes an active session.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WhisperRecognitionMode {
    Streaming,
    #[default]
    NonStreaming,
}

impl ProviderCatalog {
    pub fn whisper_recognition_mode(&self) -> Result<WhisperRecognitionMode, SettingsStoreError> {
        let Some(provider) = self
            .asr_providers
            .iter()
            .find(|provider| provider.id == WHISPER_PROVIDER_ID)
        else {
            return Ok(WhisperRecognitionMode::default());
        };
        let config = provider.config.as_object().ok_or_else(|| {
            SettingsStoreError::Invalid("Whisper configuration must be an object".into())
        })?;
        match config.get("recognition_mode") {
            None => Ok(WhisperRecognitionMode::default()),
            Some(value) => serde_json::from_value(value.clone()).map_err(|_| {
                SettingsStoreError::Invalid("unknown Whisper recognition mode".into())
            }),
        }
    }

    pub fn set_whisper_recognition_mode(
        &mut self,
        mode: WhisperRecognitionMode,
    ) -> Result<(), SettingsStoreError> {
        let value = serde_json::to_value(mode).map_err(|_| {
            SettingsStoreError::Invalid("could not encode Whisper recognition mode".into())
        })?;
        if let Some(provider) = self
            .asr_providers
            .iter_mut()
            .find(|provider| provider.id == WHISPER_PROVIDER_ID)
        {
            let object = provider.config.as_object_mut().ok_or_else(|| {
                SettingsStoreError::Invalid("Whisper configuration must be an object".into())
            })?;
            object.insert("recognition_mode".into(), value);
        } else {
            self.asr_providers.push(ProviderInstance {
                id: WHISPER_PROVIDER_ID.into(),
                name: "Whisper large-v3-turbo".into(),
                provider_type: WHISPER_PROVIDER_TYPE.into(),
                config: serde_json::json!({"recognition_mode": value}),
                data_consent: None,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::panic_in_result_fn)]
mod tests {
    use super::*;

    #[test]
    fn changing_modes_preserves_selection_and_rejects_unknown_values()
    -> Result<(), SettingsStoreError> {
        let mut catalog = ProviderCatalog::default();
        catalog.select_qwen3_asr_provider();
        let active = catalog.active.clone();
        catalog.set_whisper_recognition_mode(WhisperRecognitionMode::Streaming)?;
        assert_eq!(active, catalog.active);
        assert_eq!(
            WhisperRecognitionMode::Streaming,
            catalog.whisper_recognition_mode()?
        );
        catalog.select_whisper_provider();
        assert_eq!(
            WhisperRecognitionMode::Streaming,
            catalog.whisper_recognition_mode()?
        );
        catalog.set_whisper_recognition_mode(WhisperRecognitionMode::NonStreaming)?;
        assert_eq!(
            WhisperRecognitionMode::NonStreaming,
            catalog.whisper_recognition_mode()?
        );
        if let Some(provider) = catalog
            .asr_providers
            .iter_mut()
            .find(|provider| provider.id == WHISPER_PROVIDER_ID)
        {
            provider.config["recognition_mode"] = serde_json::json!("unknown");
        }
        assert!(catalog.whisper_recognition_mode().is_err());
        if let Some(provider) = catalog
            .asr_providers
            .iter_mut()
            .find(|provider| provider.id == WHISPER_PROVIDER_ID)
        {
            provider.config = serde_json::json!(42);
        }
        assert!(catalog.whisper_recognition_mode().is_err());
        Ok(())
    }
}
