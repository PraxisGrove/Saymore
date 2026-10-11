//! One-click cloud speech recognition presets (OpenAI, Groq).
//!
//! Both speak the OpenAI `/audio/transcriptions` protocol, so they reuse the
//! existing OpenAI-compatible recognizer. API keys are never hard-coded: they
//! come from the user's saved settings, falling back to an environment variable.

use crate::OpenAiCompatibleAsrSettings;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CloudAsrPreset {
    OpenAi,
    Groq,
}

impl CloudAsrPreset {
    pub const ALL: [Self; 2] = [Self::OpenAi, Self::Groq];

    pub fn id(self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::Groq => "groq",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::OpenAi => "OpenAI",
            Self::Groq => "Groq",
        }
    }

    pub fn base_url(self) -> &'static str {
        match self {
            Self::OpenAi => "https://api.openai.com/v1",
            Self::Groq => "https://api.groq.com/openai/v1",
        }
    }

    pub fn default_model(self) -> &'static str {
        match self {
            Self::OpenAi => "gpt-4o-mini-transcribe",
            Self::Groq => "whisper-large-v3-turbo",
        }
    }

    pub fn models(self) -> &'static [&'static str] {
        match self {
            Self::OpenAi => &["gpt-4o-mini-transcribe", "gpt-4o-transcribe", "whisper-1"],
            Self::Groq => &["whisper-large-v3-turbo", "whisper-large-v3"],
        }
    }

    /// Environment variables checked, in order, when no key is saved.
    pub fn api_key_env_vars(self) -> &'static [&'static str] {
        match self {
            Self::OpenAi => &["SAYMORE_OPENAI_API_KEY", "OPENAI_API_KEY"],
            Self::Groq => &["SAYMORE_GROQ_API_KEY", "GROQ_API_KEY"],
        }
    }

    pub fn api_key_url(self) -> &'static str {
        match self {
            Self::OpenAi => "https://platform.openai.com/api-keys",
            Self::Groq => "https://console.groq.com/keys",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|preset| preset.id() == id)
    }

    /// Detects a preset from a configured base URL (trailing slash tolerant).
    pub fn from_base_url(base_url: &str) -> Option<Self> {
        let normalized = base_url.trim().trim_end_matches('/');
        Self::ALL
            .into_iter()
            .find(|preset| preset.base_url() == normalized)
    }

    /// Resolves the API key: saved value first, then the environment.
    pub fn resolve_api_key(self, saved: &str) -> Option<String> {
        self.resolve_api_key_with(saved, |name| std::env::var(name).ok())
    }

    pub fn resolve_api_key_with(
        self,
        saved: &str,
        env: impl Fn(&str) -> Option<String>,
    ) -> Option<String> {
        let saved = saved.trim();
        if !saved.is_empty() {
            return Some(saved.to_owned());
        }
        self.api_key_env_vars()
            .iter()
            .filter_map(|name| env(name))
            .map(|value| value.trim().to_owned())
            .find(|value| !value.is_empty())
    }

    /// Builds recognizer settings. `model` falls back to the preset default.
    pub fn settings(
        self,
        saved_api_key: &str,
        model: Option<&str>,
    ) -> Option<OpenAiCompatibleAsrSettings> {
        self.settings_with(saved_api_key, model, |name| std::env::var(name).ok())
    }

    pub fn settings_with(
        self,
        saved_api_key: &str,
        model: Option<&str>,
        env: impl Fn(&str) -> Option<String>,
    ) -> Option<OpenAiCompatibleAsrSettings> {
        let api_key = self.resolve_api_key_with(saved_api_key, env)?;
        let model = model
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .unwrap_or(self.default_model());
        Some(OpenAiCompatibleAsrSettings {
            enabled: true,
            base_url: self.base_url().to_owned(),
            api_key,
            model: model.to_owned(),
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn no_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn saved_key_wins_over_env() {
        let key = CloudAsrPreset::Groq.resolve_api_key_with(" saved ", |_| Some("env".into()));
        assert_eq!(key.as_deref(), Some("saved"));
    }

    #[test]
    fn falls_back_to_env_in_order() {
        let env = |name: &str| (name == "OPENAI_API_KEY").then(|| "sk-env".to_owned());
        assert_eq!(
            CloudAsrPreset::OpenAi
                .resolve_api_key_with("", env)
                .as_deref(),
            Some("sk-env")
        );
        assert_eq!(CloudAsrPreset::Groq.resolve_api_key_with("", env), None);
    }

    #[test]
    fn builds_settings_with_default_model() {
        let s = CloudAsrPreset::Groq
            .settings_with("gsk", None, no_env)
            .unwrap();
        assert_eq!(s.base_url, "https://api.groq.com/openai/v1");
        assert_eq!(s.model, "whisper-large-v3-turbo");
        assert!(s.enabled);
        assert!(
            CloudAsrPreset::OpenAi
                .settings_with("", None, no_env)
                .is_none()
        );
    }

    #[test]
    fn detects_preset_from_url() {
        assert_eq!(
            CloudAsrPreset::from_base_url("https://api.openai.com/v1/"),
            Some(CloudAsrPreset::OpenAi)
        );
        assert_eq!(CloudAsrPreset::from_id("groq"), Some(CloudAsrPreset::Groq));
        assert_eq!(CloudAsrPreset::from_base_url("https://example.com"), None);
    }
}
