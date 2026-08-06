use template_app::{
    PARAFORMER_PROVIDER_TYPE, ProviderCatalog, QWEN3_ASR_PROVIDER_TYPE, SENSE_VOICE_PROVIDER_TYPE,
    WHISPER_PROVIDER_TYPE,
};
use template_infra::{
    PARAFORMER_MODEL_ID, PARAFORMER_MODEL_REVISION, QWEN3_ASR_MODEL_ID, QWEN3_ASR_MODEL_REVISION,
    SENSE_VOICE_MODEL_ID, SENSE_VOICE_MODEL_REVISION, WHISPER_MODEL_ID, WHISPER_MODEL_REVISION,
};

use crate::ui::AsrProviderCardKind;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub(super) enum LocalModel {
    Paraformer,
    Whisper,
    Qwen3,
    SenseVoice,
}

impl LocalModel {
    pub(super) const ALL: [Self; 4] = [
        Self::Paraformer,
        Self::Whisper,
        Self::Qwen3,
        Self::SenseVoice,
    ];

    pub(super) fn from_card(kind: AsrProviderCardKind) -> Option<Self> {
        match kind {
            AsrProviderCardKind::Paraformer => Some(Self::Paraformer),
            AsrProviderCardKind::WhisperLargeV3Turbo => Some(Self::Whisper),
            AsrProviderCardKind::Qwen3Asr => Some(Self::Qwen3),
            AsrProviderCardKind::SenseVoiceSmall => Some(Self::SenseVoice),
            _ => None,
        }
    }

    pub(super) fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|model| model.id() == id)
    }

    pub(super) fn card_kind(self) -> AsrProviderCardKind {
        match self {
            Self::Paraformer => AsrProviderCardKind::Paraformer,
            Self::Whisper => AsrProviderCardKind::WhisperLargeV3Turbo,
            Self::Qwen3 => AsrProviderCardKind::Qwen3Asr,
            Self::SenseVoice => AsrProviderCardKind::SenseVoiceSmall,
        }
    }

    pub(super) fn id(self) -> &'static str {
        match self {
            Self::Paraformer => PARAFORMER_MODEL_ID,
            Self::Whisper => WHISPER_MODEL_ID,
            Self::Qwen3 => QWEN3_ASR_MODEL_ID,
            Self::SenseVoice => SENSE_VOICE_MODEL_ID,
        }
    }

    pub(super) fn revision(self) -> &'static str {
        match self {
            Self::Paraformer => PARAFORMER_MODEL_REVISION,
            Self::Whisper => WHISPER_MODEL_REVISION,
            Self::Qwen3 => QWEN3_ASR_MODEL_REVISION,
            Self::SenseVoice => SENSE_VOICE_MODEL_REVISION,
        }
    }

    pub(super) fn provider_type(self) -> &'static str {
        match self {
            Self::Paraformer => PARAFORMER_PROVIDER_TYPE,
            Self::Whisper => WHISPER_PROVIDER_TYPE,
            Self::Qwen3 => QWEN3_ASR_PROVIDER_TYPE,
            Self::SenseVoice => SENSE_VOICE_PROVIDER_TYPE,
        }
    }

    pub(super) fn installed_name(self) -> &'static str {
        match self {
            Self::Paraformer => "Paraformer bilingual zh-en INT8",
            Self::Whisper => "Whisper large-v3-turbo INT8",
            Self::Qwen3 => "Qwen3-ASR 1.7B INT8",
            Self::SenseVoice => "SenseVoiceSmall INT8",
        }
    }

    pub(super) fn select(self, catalog: &mut ProviderCatalog) {
        match self {
            Self::Paraformer => catalog.select_paraformer_provider(),
            Self::Whisper => catalog.select_whisper_provider(),
            Self::Qwen3 => catalog.select_qwen3_asr_provider(),
            Self::SenseVoice => catalog.select_sense_voice_provider(),
        }
    }

    pub(super) fn is_active(self, catalog: &ProviderCatalog) -> bool {
        match self {
            Self::Paraformer => catalog.paraformer_is_active(),
            Self::Whisper => catalog.whisper_is_active(),
            Self::Qwen3 => catalog.qwen3_asr_is_active(),
            Self::SenseVoice => catalog.sense_voice_is_active(),
        }
    }

    pub(super) fn clear_selection(self, catalog: &mut ProviderCatalog) -> bool {
        match self {
            Self::Paraformer => catalog.clear_paraformer_selection(),
            Self::Whisper => catalog.clear_whisper_selection(),
            Self::Qwen3 => catalog.clear_qwen3_asr_selection(),
            Self::SenseVoice => catalog.clear_sense_voice_selection(),
        }
    }

    pub(super) fn thread_name(self, operation: &str) -> String {
        format!("saymore-{operation}-{}", self.id())
    }
}
