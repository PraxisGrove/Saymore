#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LocalModelStorageUsage {
    pub paraformer_bytes: u64,
    pub whisper_bytes: u64,
    pub qwen3_asr_bytes: u64,
    pub sense_voice_bytes: u64,
    pub punctuation_bytes: u64,
}

impl LocalModelStorageUsage {
    pub fn total_bytes(self) -> u64 {
        self.paraformer_bytes
            .saturating_add(self.whisper_bytes)
            .saturating_add(self.qwen3_asr_bytes)
            .saturating_add(self.sense_voice_bytes)
            .saturating_add(self.punctuation_bytes)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LocalStorageUsage {
    pub total_bytes: u64,
    pub local_models_bytes: u64,
    pub recognition_data_bytes: u64,
    pub diagnostic_logs_bytes: u64,
    pub configuration_other_bytes: u64,
    pub models: LocalModelStorageUsage,
}
