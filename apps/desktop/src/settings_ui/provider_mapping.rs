use crate::ui::LlmProvider as UiLlmProvider;
use template_app::LlmProviderPreset;

pub(super) const VOLCENGINE_ASR_1_MODEL: &str = "volc.bigasr.sauc.duration";
pub(super) const VOLCENGINE_ASR_2_MODEL: &str = "volc.seedasr.sauc.duration";
pub(super) const VOLCENGINE_LEGACY_MODEL: &str = "bigmodel_async";
pub(super) const VOLCENGINE_MODELS: [&str; 2] = [VOLCENGINE_ASR_2_MODEL, VOLCENGINE_ASR_1_MODEL];

pub(super) fn provider_preset(provider: UiLlmProvider) -> LlmProviderPreset {
    match provider {
        UiLlmProvider::Sensenova => LlmProviderPreset::SenseNova,
        UiLlmProvider::Deepseek => LlmProviderPreset::DeepSeek,
        UiLlmProvider::Qwen => LlmProviderPreset::Qwen,
        UiLlmProvider::VolcengineArk => LlmProviderPreset::VolcengineArk,
        UiLlmProvider::Openai => LlmProviderPreset::OpenAi,
        UiLlmProvider::Kimi => LlmProviderPreset::Kimi,
        UiLlmProvider::Gemini => LlmProviderPreset::Gemini,
        UiLlmProvider::Openrouter => LlmProviderPreset::OpenRouter,
        UiLlmProvider::ZhipuGlm => LlmProviderPreset::ZhipuGlm,
        UiLlmProvider::Minimax => LlmProviderPreset::MiniMax,
        UiLlmProvider::Siliconflow => LlmProviderPreset::SiliconFlow,
        UiLlmProvider::Stepfun => LlmProviderPreset::StepFun,
        UiLlmProvider::Custom => LlmProviderPreset::Custom,
    }
}

pub(super) fn ui_provider(provider: LlmProviderPreset) -> UiLlmProvider {
    match provider {
        LlmProviderPreset::SenseNova => UiLlmProvider::Sensenova,
        LlmProviderPreset::DeepSeek => UiLlmProvider::Deepseek,
        LlmProviderPreset::Qwen => UiLlmProvider::Qwen,
        LlmProviderPreset::VolcengineArk => UiLlmProvider::VolcengineArk,
        LlmProviderPreset::OpenAi => UiLlmProvider::Openai,
        LlmProviderPreset::Kimi => UiLlmProvider::Kimi,
        LlmProviderPreset::Gemini => UiLlmProvider::Gemini,
        LlmProviderPreset::OpenRouter => UiLlmProvider::Openrouter,
        LlmProviderPreset::ZhipuGlm => UiLlmProvider::ZhipuGlm,
        LlmProviderPreset::MiniMax => UiLlmProvider::Minimax,
        LlmProviderPreset::SiliconFlow => UiLlmProvider::Siliconflow,
        LlmProviderPreset::StepFun => UiLlmProvider::Stepfun,
        LlmProviderPreset::Custom => UiLlmProvider::Custom,
    }
}
