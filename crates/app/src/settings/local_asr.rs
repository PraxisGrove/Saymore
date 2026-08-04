use super::*;

impl ProviderCatalog {
    pub fn select_sense_voice_provider(&mut self) {
        if !self
            .asr_providers
            .iter()
            .any(|provider| provider.id == SENSE_VOICE_PROVIDER_ID)
        {
            self.asr_providers.push(ProviderInstance {
                id: SENSE_VOICE_PROVIDER_ID.to_owned(),
                name: "SenseVoiceSmall INT8".to_owned(),
                provider_type: SENSE_VOICE_PROVIDER_TYPE.to_owned(),
                config: serde_json::json!({}),
                data_consent: None,
            });
        }
        self.active.asr = Some(SENSE_VOICE_PROVIDER_ID.to_owned());
    }

    pub fn sense_voice_is_active(&self) -> bool {
        self.active.asr.as_deref() == Some(SENSE_VOICE_PROVIDER_ID)
            && self.asr_providers.iter().any(|provider| {
                provider.id == SENSE_VOICE_PROVIDER_ID
                    && provider.provider_type == SENSE_VOICE_PROVIDER_TYPE
            })
    }

    pub fn clear_sense_voice_selection(&mut self) -> bool {
        if self.active.asr.as_deref() != Some(SENSE_VOICE_PROVIDER_ID) {
            return false;
        }
        self.active.asr = None;
        true
    }
}
