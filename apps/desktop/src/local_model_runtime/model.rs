use template_app::{ProviderCatalog, QWEN3_ASR_PROVIDER_TYPE, WHISPER_PROVIDER_TYPE};
use template_app::{SEACO_PROVIDER_TYPE, ZIPFORMER_PROVIDER_TYPE};
use template_infra::{
    QWEN3_ASR_MODEL_ID, QWEN3_ASR_MODEL_REVISION, WHISPER_MODEL_ID, WHISPER_MODEL_REVISION,
};
use template_infra::{
    SEACO_MODEL_ID, SEACO_MODEL_REVISION, ZIPFORMER_MODEL_ID, ZIPFORMER_MODEL_REVISION,
};

use crate::ui::AsrProviderCardKind;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub(super) enum LocalModel {
    Whisper,
    Qwen3,
    Seaco,
    Zipformer,
}

impl LocalModel {
    pub(super) const ALL: [Self; 4] = [Self::Whisper, Self::Qwen3, Self::Seaco, Self::Zipformer];

    pub(super) fn from_card(kind: AsrProviderCardKind) -> Option<Self> {
        match kind {
            AsrProviderCardKind::WhisperLargeV3Turbo => Some(Self::Whisper),
            AsrProviderCardKind::Qwen3Asr => Some(Self::Qwen3),
            AsrProviderCardKind::SeacoParaformer => Some(Self::Seaco),
            AsrProviderCardKind::Zipformer => Some(Self::Zipformer),
            _ => None,
        }
    }

    pub(super) fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|model| model.id() == id)
    }

    pub(super) fn card_kind(self) -> AsrProviderCardKind {
        match self {
            Self::Whisper => AsrProviderCardKind::WhisperLargeV3Turbo,
            Self::Qwen3 => AsrProviderCardKind::Qwen3Asr,
            Self::Seaco => AsrProviderCardKind::SeacoParaformer,
            Self::Zipformer => AsrProviderCardKind::Zipformer,
        }
    }

    pub(super) fn id(self) -> &'static str {
        match self {
            Self::Whisper => WHISPER_MODEL_ID,
            Self::Qwen3 => QWEN3_ASR_MODEL_ID,
            Self::Seaco => SEACO_MODEL_ID,
            Self::Zipformer => ZIPFORMER_MODEL_ID,
        }
    }

    pub(super) fn revision(self) -> &'static str {
        match self {
            Self::Whisper => WHISPER_MODEL_REVISION,
            Self::Qwen3 => QWEN3_ASR_MODEL_REVISION,
            Self::Seaco => SEACO_MODEL_REVISION,
            Self::Zipformer => ZIPFORMER_MODEL_REVISION,
        }
    }

    pub(super) fn provider_type(self) -> &'static str {
        match self {
            Self::Whisper => WHISPER_PROVIDER_TYPE,
            Self::Qwen3 => QWEN3_ASR_PROVIDER_TYPE,
            Self::Seaco => SEACO_PROVIDER_TYPE,
            Self::Zipformer => ZIPFORMER_PROVIDER_TYPE,
        }
    }

    pub(super) fn installed_name(self) -> &'static str {
        match self {
            Self::Whisper => "Whisper large-v3-turbo Q8_0",
            Self::Qwen3 => "Qwen3-ASR 1.7B INT8",
            Self::Seaco => "SeACo-Paraformer FP32",
            Self::Zipformer => "Zipformer bilingual INT8 encoder",
        }
    }

    pub(super) fn select(self, catalog: &mut ProviderCatalog) {
        match self {
            Self::Whisper => catalog.select_whisper_provider(),
            Self::Qwen3 => catalog.select_qwen3_asr_provider(),
            Self::Seaco => catalog.select_seaco_provider(),
            Self::Zipformer => catalog.select_zipformer_provider(),
        }
    }

    pub(super) fn is_active(self, catalog: &ProviderCatalog) -> bool {
        match self {
            Self::Whisper => catalog.whisper_is_active(),
            Self::Qwen3 => catalog.qwen3_asr_is_active(),
            Self::Seaco => catalog.seaco_is_active(),
            Self::Zipformer => catalog.zipformer_is_active(),
        }
    }

    pub(super) fn clear_selection(self, catalog: &mut ProviderCatalog) -> bool {
        match self {
            Self::Whisper => catalog.clear_whisper_selection(),
            Self::Qwen3 => catalog.clear_qwen3_asr_selection(),
            Self::Seaco => catalog.clear_seaco_selection(),
            Self::Zipformer => catalog.clear_zipformer_selection(),
        }
    }

    pub(super) fn thread_name(self, operation: &str) -> String {
        format!("saymore-{operation}-{}", self.id())
    }
}
