use std::collections::BTreeMap;

use super::ChatCompletionsLlmSettings;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChatCompletionsProfile {
    #[default]
    Portable,
    DeepSeek,
    VolcengineArk,
    Qwen,
    ZhipuGlm,
    MiniMax,
    Kimi,
    OpenRouter,
}

impl ChatCompletionsProfile {
    pub const fn id(self) -> &'static str {
        match self {
            Self::Portable => "portable",
            Self::DeepSeek => "deepseek",
            Self::VolcengineArk => "volcengine_ark",
            Self::Qwen => "qwen",
            Self::ZhipuGlm => "zhipu_glm",
            Self::MiniMax => "minimax",
            Self::Kimi => "kimi",
            Self::OpenRouter => "openrouter",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "portable" => Some(Self::Portable),
            "deepseek" => Some(Self::DeepSeek),
            "volcengine_ark" => Some(Self::VolcengineArk),
            "qwen" => Some(Self::Qwen),
            "zhipu_glm" => Some(Self::ZhipuGlm),
            "minimax" => Some(Self::MiniMax),
            "kimi" => Some(Self::Kimi),
            "openrouter" => Some(Self::OpenRouter),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LlmProviderProfile {
    pub id: &'static str,
    pub label: &'static str,
    pub base_url: &'static str,
    pub default_model: &'static str,
    pub model_list_url: &'static str,
    pub recommended_models: &'static [&'static str],
    pub base_url_editable: bool,
    pub api_key_environment: &'static str,
    pub chat_completions: ChatCompletionsProfile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmProviderPreset {
    SenseNova,
    DeepSeek,
    Qwen,
    VolcengineArk,
    OpenAi,
    Kimi,
    Gemini,
    OpenRouter,
    ZhipuGlm,
    MiniMax,
    SiliconFlow,
    StepFun,
    Custom,
}

impl LlmProviderPreset {
    pub const ALL: [Self; 13] = [
        Self::SenseNova,
        Self::DeepSeek,
        Self::Qwen,
        Self::VolcengineArk,
        Self::OpenAi,
        Self::Kimi,
        Self::Gemini,
        Self::OpenRouter,
        Self::ZhipuGlm,
        Self::MiniMax,
        Self::SiliconFlow,
        Self::StepFun,
        Self::Custom,
    ];

    pub(super) const BUILT_INS: [Self; 12] = [
        Self::SenseNova,
        Self::DeepSeek,
        Self::Qwen,
        Self::VolcengineArk,
        Self::OpenAi,
        Self::Kimi,
        Self::Gemini,
        Self::OpenRouter,
        Self::ZhipuGlm,
        Self::MiniMax,
        Self::SiliconFlow,
        Self::StepFun,
    ];

    pub const fn profile(self) -> &'static LlmProviderProfile {
        match self {
            Self::SenseNova => &SENSENOVA_PROFILE,
            Self::DeepSeek => &DEEPSEEK_PROFILE,
            Self::Qwen => &QWEN_PROFILE,
            Self::VolcengineArk => &VOLCENGINE_ARK_PROFILE,
            Self::OpenAi => &OPENAI_PROFILE,
            Self::Kimi => &KIMI_PROFILE,
            Self::Gemini => &GEMINI_PROFILE,
            Self::OpenRouter => &OPENROUTER_PROFILE,
            Self::ZhipuGlm => &ZHIPU_GLM_PROFILE,
            Self::MiniMax => &MINIMAX_PROFILE,
            Self::SiliconFlow => &SILICONFLOW_PROFILE,
            Self::StepFun => &STEPFUN_PROFILE,
            Self::Custom => &CUSTOM_PROFILE,
        }
    }

    pub const fn label(self) -> &'static str {
        self.profile().label
    }

    pub const fn id(self) -> &'static str {
        self.profile().id
    }

    pub const fn base_url(self) -> &'static str {
        self.profile().base_url
    }

    pub const fn model(self) -> &'static str {
        self.profile().default_model
    }

    pub const fn model_list_url(self) -> &'static str {
        self.profile().model_list_url
    }

    pub const fn recommended_models(self) -> &'static [&'static str] {
        self.profile().recommended_models
    }

    pub const fn supports_remote_model_discovery(self) -> bool {
        !self.profile().model_list_url.is_empty()
    }

    pub const fn base_url_editable(self) -> bool {
        self.profile().base_url_editable
    }

    pub fn from_id_or_base_url(id: &str, base_url: &str) -> Option<Self> {
        Self::BUILT_INS.into_iter().find(|preset| {
            preset.id() == id
                || preset.base_url().trim_end_matches('/') == base_url.trim_end_matches('/')
        })
    }

    pub fn settings(self, api_key: &str) -> ChatCompletionsLlmSettings {
        ChatCompletionsLlmSettings {
            base_url: self.base_url().to_owned(),
            api_key: api_key.trim().to_owned(),
            model: self.model().to_owned(),
            custom_headers: BTreeMap::new(),
            profile: self.profile().chat_completions,
        }
    }
}

const SENSENOVA_PROFILE: LlmProviderProfile = LlmProviderProfile {
    id: "sensenova",
    label: "商汤 SenseNova",
    base_url: "https://token.sensenova.cn/v1",
    default_model: "sensenova-6.7-flash-lite",
    model_list_url: "https://api.sensenova.cn/v1/llm/models",
    recommended_models: &[],
    base_url_editable: false,
    api_key_environment: "SENSENOVA_API_KEY",
    chat_completions: ChatCompletionsProfile::Portable,
};
const DEEPSEEK_PROFILE: LlmProviderProfile = LlmProviderProfile {
    id: "deepseek",
    label: "DeepSeek",
    base_url: "https://api.deepseek.com",
    default_model: "deepseek-v4-flash",
    model_list_url: "https://api.deepseek.com/models",
    recommended_models: &[],
    base_url_editable: false,
    api_key_environment: "DEEPSEEK_API_KEY",
    chat_completions: ChatCompletionsProfile::DeepSeek,
};
const QWEN_PROFILE: LlmProviderProfile = LlmProviderProfile {
    id: "qwen",
    label: "通义千问",
    base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1",
    default_model: "qwen-plus",
    model_list_url: "",
    recommended_models: &["qwen-plus", "qwen-flash"],
    base_url_editable: true,
    api_key_environment: "DASHSCOPE_API_KEY",
    chat_completions: ChatCompletionsProfile::Qwen,
};
const VOLCENGINE_ARK_PROFILE: LlmProviderProfile = LlmProviderProfile {
    id: "volcengine_ark",
    label: "火山方舟",
    base_url: "https://ark.cn-beijing.volces.com/api/v3",
    default_model: "doubao-seed-2-0-lite-260215",
    model_list_url: "",
    recommended_models: &["doubao-seed-2-0-lite-260215"],
    base_url_editable: false,
    api_key_environment: "ARK_API_KEY",
    chat_completions: ChatCompletionsProfile::VolcengineArk,
};
const OPENAI_PROFILE: LlmProviderProfile = LlmProviderProfile {
    id: "openai",
    label: "OpenAI",
    base_url: "https://api.openai.com/v1",
    default_model: "gpt-5.6-sol",
    model_list_url: "https://api.openai.com/v1/models",
    recommended_models: &[],
    base_url_editable: false,
    api_key_environment: "OPENAI_API_KEY",
    chat_completions: ChatCompletionsProfile::Portable,
};
const KIMI_PROFILE: LlmProviderProfile = LlmProviderProfile {
    id: "kimi",
    label: "Kimi",
    base_url: "https://api.moonshot.cn/v1",
    default_model: "kimi-k2.6",
    model_list_url: "https://api.moonshot.cn/v1/models",
    recommended_models: &[],
    base_url_editable: false,
    api_key_environment: "MOONSHOT_API_KEY",
    chat_completions: ChatCompletionsProfile::Kimi,
};
const GEMINI_PROFILE: LlmProviderProfile = LlmProviderProfile {
    id: "gemini",
    label: "Gemini",
    base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
    default_model: "gemini-3.6-flash",
    model_list_url: "https://generativelanguage.googleapis.com/v1beta/openai/models",
    recommended_models: &[],
    base_url_editable: false,
    api_key_environment: "GEMINI_API_KEY",
    chat_completions: ChatCompletionsProfile::Portable,
};
const OPENROUTER_PROFILE: LlmProviderProfile = LlmProviderProfile {
    id: "openrouter",
    label: "OpenRouter",
    base_url: "https://openrouter.ai/api/v1",
    default_model: "openrouter/auto",
    model_list_url: "https://openrouter.ai/api/v1/models",
    recommended_models: &[],
    base_url_editable: false,
    api_key_environment: "OPENROUTER_API_KEY",
    chat_completions: ChatCompletionsProfile::OpenRouter,
};
const ZHIPU_GLM_PROFILE: LlmProviderProfile = LlmProviderProfile {
    id: "zhipu_glm",
    label: "智谱 GLM",
    base_url: "https://open.bigmodel.cn/api/paas/v4",
    default_model: "glm-4.7-flash",
    model_list_url: "",
    recommended_models: &["glm-4.7-flash", "glm-5.2"],
    base_url_editable: false,
    api_key_environment: "ZHIPU_API_KEY",
    chat_completions: ChatCompletionsProfile::ZhipuGlm,
};
const MINIMAX_PROFILE: LlmProviderProfile = LlmProviderProfile {
    id: "minimax",
    label: "MiniMax",
    base_url: "https://api.minimaxi.com/v1",
    default_model: "MiniMax-M3",
    model_list_url: "https://api.minimaxi.com/v1/models",
    recommended_models: &["MiniMax-M3", "MiniMax-M2.7"],
    base_url_editable: false,
    api_key_environment: "MINIMAX_API_KEY",
    chat_completions: ChatCompletionsProfile::MiniMax,
};
const SILICONFLOW_PROFILE: LlmProviderProfile = LlmProviderProfile {
    id: "siliconflow",
    label: "硅基流动",
    base_url: "https://api.siliconflow.cn/v1",
    default_model: "Pro/zai-org/GLM-4.7",
    model_list_url: "https://api.siliconflow.cn/v1/models?type=text&sub_type=chat",
    recommended_models: &[],
    base_url_editable: false,
    api_key_environment: "SILICONFLOW_API_KEY",
    chat_completions: ChatCompletionsProfile::Portable,
};
const STEPFUN_PROFILE: LlmProviderProfile = LlmProviderProfile {
    id: "stepfun",
    label: "阶跃星辰",
    base_url: "https://api.stepfun.com/v1",
    default_model: "step-3.5-flash",
    model_list_url: "https://api.stepfun.com/v1/models",
    recommended_models: &[],
    base_url_editable: false,
    api_key_environment: "STEP_API_KEY",
    chat_completions: ChatCompletionsProfile::Portable,
};
const CUSTOM_PROFILE: LlmProviderProfile = LlmProviderProfile {
    id: "custom",
    label: "Custom compatible API",
    base_url: "",
    default_model: "",
    model_list_url: "",
    recommended_models: &[],
    base_url_editable: true,
    api_key_environment: "SAYMORE_LLM_API_KEY",
    chat_completions: ChatCompletionsProfile::Portable,
};
