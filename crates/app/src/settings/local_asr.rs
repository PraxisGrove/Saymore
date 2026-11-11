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

    pub fn select_whisper_provider(&mut self) {
        if !self
            .asr_providers
            .iter()
            .any(|provider| provider.id == WHISPER_PROVIDER_ID)
        {
            self.asr_providers.push(ProviderInstance {
                id: WHISPER_PROVIDER_ID.to_owned(),
                name: "Whisper large-v3-turbo".to_owned(),
                provider_type: WHISPER_PROVIDER_TYPE.to_owned(),
                config: serde_json::json!({}),
                data_consent: None,
            });
        }
        self.active.asr = Some(WHISPER_PROVIDER_ID.to_owned());
    }

    pub fn whisper_is_active(&self) -> bool {
        self.active.asr.as_deref() == Some(WHISPER_PROVIDER_ID)
            && self.asr_providers.iter().any(|provider| {
                provider.id == WHISPER_PROVIDER_ID
                    && provider.provider_type == WHISPER_PROVIDER_TYPE
            })
    }

    pub fn clear_whisper_selection(&mut self) -> bool {
        if self.active.asr.as_deref() != Some(WHISPER_PROVIDER_ID) {
            return false;
        }
        self.active.asr = None;
        true
    }

    pub fn select_qwen3_asr_provider(&mut self) {
        if !self
            .asr_providers
            .iter()
            .any(|provider| provider.id == QWEN3_ASR_PROVIDER_ID)
        {
            self.asr_providers.push(ProviderInstance {
                id: QWEN3_ASR_PROVIDER_ID.to_owned(),
                name: "Qwen3-ASR 1.7B INT8".to_owned(),
                provider_type: QWEN3_ASR_PROVIDER_TYPE.to_owned(),
                config: serde_json::json!({}),
                data_consent: None,
            });
        }
        self.active.asr = Some(QWEN3_ASR_PROVIDER_ID.to_owned());
    }

    pub fn qwen3_asr_is_active(&self) -> bool {
        self.active.asr.as_deref() == Some(QWEN3_ASR_PROVIDER_ID)
            && self.asr_providers.iter().any(|provider| {
                provider.id == QWEN3_ASR_PROVIDER_ID
                    && provider.provider_type == QWEN3_ASR_PROVIDER_TYPE
            })
    }

    pub fn clear_qwen3_asr_selection(&mut self) -> bool {
        if self.active.asr.as_deref() != Some(QWEN3_ASR_PROVIDER_ID) {
            return false;
        }
        self.active.asr = None;
        true
    }
}

impl ProviderCatalog {
    pub fn select_seaco_provider(&mut self) {
        self.select_local_provider(SEACO_PROVIDER_ID, SEACO_PROVIDER_TYPE, "SeACo-Paraformer");
    }

    pub fn seaco_is_active(&self) -> bool {
        self.local_provider_is_active(SEACO_PROVIDER_ID, SEACO_PROVIDER_TYPE)
    }

    pub fn clear_seaco_selection(&mut self) -> bool {
        self.clear_local_selection(SEACO_PROVIDER_ID)
    }
}

impl ProviderCatalog {
    pub fn select_zipformer_provider(&mut self) {
        self.select_local_provider(ZIPFORMER_PROVIDER_ID, ZIPFORMER_PROVIDER_TYPE, "Zipformer");
    }

    pub fn zipformer_is_active(&self) -> bool {
        self.local_provider_is_active(ZIPFORMER_PROVIDER_ID, ZIPFORMER_PROVIDER_TYPE)
    }

    pub fn clear_zipformer_selection(&mut self) -> bool {
        self.clear_local_selection(ZIPFORMER_PROVIDER_ID)
    }
}

impl ProviderCatalog {
    pub fn select_moonshine_provider(&mut self) {
        self.select_local_provider(MOONSHINE_PROVIDER_ID, MOONSHINE_PROVIDER_TYPE, "Moonshine");
    }

    pub fn moonshine_is_active(&self) -> bool {
        self.local_provider_is_active(MOONSHINE_PROVIDER_ID, MOONSHINE_PROVIDER_TYPE)
    }

    pub fn clear_moonshine_selection(&mut self) -> bool {
        self.clear_local_selection(MOONSHINE_PROVIDER_ID)
    }
}

impl ProviderCatalog {
    fn select_local_provider(&mut self, id: &str, provider_type: &str, name: &str) {
        if !self.asr_providers.iter().any(|provider| provider.id == id) {
            self.asr_providers.push(ProviderInstance {
                id: id.to_owned(),
                name: name.to_owned(),
                provider_type: provider_type.to_owned(),
                config: serde_json::json!({}),
                data_consent: None,
            });
        }
        self.active.asr = Some(id.to_owned());
    }

    fn local_provider_is_active(&self, id: &str, provider_type: &str) -> bool {
        self.active.asr.as_deref() == Some(id)
            && self
                .asr_providers
                .iter()
                .any(|provider| provider.id == id && provider.provider_type == provider_type)
    }

    fn clear_local_selection(&mut self, id: &str) -> bool {
        if self.active.asr.as_deref() != Some(id) {
            return false;
        }
        self.active.asr = None;
        true
    }
}
