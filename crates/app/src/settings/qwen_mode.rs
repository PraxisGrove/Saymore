use super::{
    ProviderCatalog, ProviderInstance, QWEN3_ASR_PROVIDER_ID, QWEN3_ASR_PROVIDER_TYPE,
    SettingsStoreError,
};
use serde::{Deserialize, Serialize};

/// Selects Qwen's cumulative-audio streaming algorithm or existing offline segments.
/// A dictation snapshots this choice at start; changing it never changes an active session.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QwenRecognitionMode {
    Streaming,
    #[default]
    NonStreaming,
}

impl ProviderCatalog {
    pub fn qwen_recognition_mode(&self) -> Result<QwenRecognitionMode, SettingsStoreError> {
        let Some(provider) = self
            .asr_providers
            .iter()
            .find(|provider| provider.id == QWEN3_ASR_PROVIDER_ID)
        else {
            return Ok(QwenRecognitionMode::default());
        };
        let config = provider.config.as_object().ok_or_else(|| {
            SettingsStoreError::Invalid("Qwen configuration must be an object".into())
        })?;
        match config.get("recognition_mode") {
            None => Ok(QwenRecognitionMode::default()),
            Some(value) => serde_json::from_value(value.clone())
                .map_err(|_| SettingsStoreError::Invalid("unknown Qwen recognition mode".into())),
        }
    }

    pub fn set_qwen_recognition_mode(
        &mut self,
        mode: QwenRecognitionMode,
    ) -> Result<(), SettingsStoreError> {
        let value = serde_json::to_value(mode).map_err(|_| {
            SettingsStoreError::Invalid("could not encode Qwen recognition mode".into())
        })?;
        if let Some(provider) = self
            .asr_providers
            .iter_mut()
            .find(|provider| provider.id == QWEN3_ASR_PROVIDER_ID)
        {
            let object = provider.config.as_object_mut().ok_or_else(|| {
                SettingsStoreError::Invalid("Qwen configuration must be an object".into())
            })?;
            object.insert("recognition_mode".into(), value);
        } else {
            self.asr_providers.push(ProviderInstance {
                id: QWEN3_ASR_PROVIDER_ID.into(),
                name: "Qwen3-ASR 1.7B Q8_0".into(),
                provider_type: QWEN3_ASR_PROVIDER_TYPE.into(),
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
        catalog.select_whisper_provider();
        let active = catalog.active.clone();
        catalog.set_qwen_recognition_mode(QwenRecognitionMode::Streaming)?;
        assert_eq!(active, catalog.active);
        assert_eq!(
            QwenRecognitionMode::Streaming,
            catalog.qwen_recognition_mode()?
        );
        catalog.select_qwen3_asr_provider();
        assert_eq!(
            QwenRecognitionMode::Streaming,
            catalog.qwen_recognition_mode()?
        );
        catalog.set_qwen_recognition_mode(QwenRecognitionMode::NonStreaming)?;
        assert_eq!(
            QwenRecognitionMode::NonStreaming,
            catalog.qwen_recognition_mode()?
        );
        if let Some(provider) = catalog
            .asr_providers
            .iter_mut()
            .find(|provider| provider.id == QWEN3_ASR_PROVIDER_ID)
        {
            provider.config["recognition_mode"] = serde_json::json!("unknown");
        }
        assert!(catalog.qwen_recognition_mode().is_err());
        if let Some(provider) = catalog
            .asr_providers
            .iter_mut()
            .find(|provider| provider.id == QWEN3_ASR_PROVIDER_ID)
        {
            provider.config = serde_json::json!(42);
        }
        assert!(catalog.qwen_recognition_mode().is_err());
        Ok(())
    }
}
