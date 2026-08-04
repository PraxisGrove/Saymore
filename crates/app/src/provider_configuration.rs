use std::net::IpAddr;

use thiserror::Error;

use crate::{
    ChatCompletionsLlmSettings, LlmProviderError, LlmProviderPreset, OpenAiCompatibleAsrSettings,
    SaymoreSettings, SettingsStoreError, SpeechRecognitionError, VolcengineAsrSettings,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AsrProviderConfiguration {
    Volcengine(VolcengineAsrSettings),
    OpenAiCompatible(OpenAiCompatibleAsrSettings),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmProviderConfiguration {
    provider: LlmProviderPreset,
    settings: ChatCompletionsLlmSettings,
}

impl LlmProviderConfiguration {
    pub fn new(provider: LlmProviderPreset, settings: ChatCompletionsLlmSettings) -> Self {
        Self { provider, settings }
    }

    pub fn provider(&self) -> LlmProviderPreset {
        self.provider
    }

    pub fn settings(&self) -> &ChatCompletionsLlmSettings {
        &self.settings
    }
}

#[derive(Debug, Error)]
pub enum AsrConfigurationError {
    #[error("ASR connection test failed")]
    Connection(#[source] SpeechRecognitionError),
    #[error("ASR configuration could not be saved")]
    Store(#[source] SettingsStoreError),
}

#[derive(Debug, Error)]
pub enum LlmConfigurationError {
    #[error("LLM connection test failed")]
    Connection(#[source] LlmProviderError),
    #[error("LLM configuration could not be saved")]
    Store(#[source] SettingsStoreError),
}

/// Tests candidate Provider configurations without persisting them.
///
/// Implementations must not mutate saved configuration. An ASR test returns the
/// transcript produced from the standard connection-test audio.
pub trait ProviderConnectionTester: Send + Sync {
    fn test_asr(
        &self,
        candidate: &AsrProviderConfiguration,
    ) -> Result<String, SpeechRecognitionError>;

    fn test_llm(&self, candidate: &LlmProviderConfiguration) -> Result<(), LlmProviderError>;
}

/// Atomically persists a Provider configuration after its connection test passes.
///
/// Implementations must leave the previous configuration unchanged on failure.
pub trait ProviderConfigurationStore: Send + Sync {
    fn save_asr_configuration(
        &self,
        candidate: &AsrProviderConfiguration,
    ) -> Result<(), SettingsStoreError>;

    fn save_and_enable_llm_configuration(
        &self,
        candidate: &LlmProviderConfiguration,
    ) -> Result<(), SettingsStoreError>;
}

pub struct ProviderConfigurator<'a> {
    tester: &'a dyn ProviderConnectionTester,
    store: &'a dyn ProviderConfigurationStore,
}

impl<'a> ProviderConfigurator<'a> {
    pub fn new(
        tester: &'a dyn ProviderConnectionTester,
        store: &'a dyn ProviderConfigurationStore,
    ) -> Self {
        Self { tester, store }
    }

    pub fn configure_asr(
        &self,
        candidate: &AsrProviderConfiguration,
    ) -> Result<String, AsrConfigurationError> {
        let transcript = self
            .tester
            .test_asr(candidate)
            .map_err(AsrConfigurationError::Connection)?;
        self.store
            .save_asr_configuration(candidate)
            .map_err(AsrConfigurationError::Store)?;
        Ok(transcript)
    }

    pub fn configure_llm(
        &self,
        candidate: &LlmProviderConfiguration,
    ) -> Result<(), LlmConfigurationError> {
        self.tester
            .test_llm(candidate)
            .map_err(LlmConfigurationError::Connection)?;
        self.store
            .save_and_enable_llm_configuration(candidate)
            .map_err(LlmConfigurationError::Store)
    }
}

pub fn llm_consent_required(settings: &SaymoreSettings, expected_base_url: &str) -> bool {
    !provider_is_local(expected_base_url)
        && settings.llm.confirmed_base_url.trim() != expected_base_url.trim()
}

pub fn provider_is_local(base_url: &str) -> bool {
    let authority = base_url
        .split_once("://")
        .map_or(base_url, |(_, remainder)| remainder)
        .split('/')
        .next()
        .map_or("", |value| value)
        .rsplit('@')
        .next()
        .map_or("", |value| value);
    let host = if authority.eq_ignore_ascii_case("::1") {
        authority
    } else if let Some(bracketed) = authority.strip_prefix('[') {
        bracketed.split(']').next().map_or("", |value| value)
    } else {
        authority.split(':').next().map_or("", |value| value)
    };
    let host = host.trim_end_matches('.');
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, MutexGuard};

    use super::*;

    #[derive(Default)]
    struct FakeAdapter {
        asr_test_result: Mutex<Option<Result<String, SpeechRecognitionError>>>,
        llm_test_result: Mutex<Option<Result<(), LlmProviderError>>>,
        saved_asr: Mutex<Vec<AsrProviderConfiguration>>,
        saved_llm: Mutex<Vec<LlmProviderConfiguration>>,
    }

    fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
        match mutex.lock() {
            Ok(guard) => guard,
            Err(_) => panic!("test lock should be available"),
        }
    }

    fn take_result<T>(mutex: &Mutex<Option<T>>) -> T {
        match lock(mutex).take() {
            Some(result) => result,
            None => panic!("test result should be configured"),
        }
    }

    impl ProviderConnectionTester for FakeAdapter {
        fn test_asr(
            &self,
            _candidate: &AsrProviderConfiguration,
        ) -> Result<String, SpeechRecognitionError> {
            take_result(&self.asr_test_result)
        }

        fn test_llm(&self, _candidate: &LlmProviderConfiguration) -> Result<(), LlmProviderError> {
            take_result(&self.llm_test_result)
        }
    }

    impl ProviderConfigurationStore for FakeAdapter {
        fn save_asr_configuration(
            &self,
            candidate: &AsrProviderConfiguration,
        ) -> Result<(), SettingsStoreError> {
            lock(&self.saved_asr).push(candidate.clone());
            Ok(())
        }

        fn save_and_enable_llm_configuration(
            &self,
            candidate: &LlmProviderConfiguration,
        ) -> Result<(), SettingsStoreError> {
            lock(&self.saved_llm).push(candidate.clone());
            Ok(())
        }
    }

    fn asr_candidate() -> AsrProviderConfiguration {
        AsrProviderConfiguration::Volcengine(VolcengineAsrSettings {
            enabled: true,
            api_key: "candidate-key".to_owned(),
            model: "candidate-model".to_owned(),
        })
    }

    fn llm_candidate() -> LlmProviderConfiguration {
        LlmProviderConfiguration::new(
            LlmProviderPreset::DeepSeek,
            LlmProviderPreset::DeepSeek.settings("candidate-key"),
        )
    }

    #[test]
    fn failed_connection_tests_do_not_persist_candidates() {
        let adapter = FakeAdapter::default();
        *lock(&adapter.asr_test_result) = Some(Err(SpeechRecognitionError::Authentication));
        *lock(&adapter.llm_test_result) = Some(Err(LlmProviderError::Transport(
            "connection test unavailable".to_owned(),
        )));
        let configurator = ProviderConfigurator::new(&adapter, &adapter);

        assert!(configurator.configure_asr(&asr_candidate()).is_err());
        assert!(configurator.configure_llm(&llm_candidate()).is_err());
        assert!(lock(&adapter.saved_asr).is_empty());
        assert!(lock(&adapter.saved_llm).is_empty());
    }

    #[test]
    fn successful_connection_tests_persist_candidates_and_return_asr_transcript() {
        let adapter = FakeAdapter::default();
        *lock(&adapter.asr_test_result) = Some(Ok("test succeeded".to_owned()));
        *lock(&adapter.llm_test_result) = Some(Ok(()));
        let configurator = ProviderConfigurator::new(&adapter, &adapter);

        assert!(matches!(
            configurator.configure_asr(&asr_candidate()),
            Ok(transcript) if transcript == "test succeeded"
        ));
        assert!(configurator.configure_llm(&llm_candidate()).is_ok());
        assert_eq!(vec![asr_candidate()], *lock(&adapter.saved_asr));
        assert_eq!(vec![llm_candidate()], *lock(&adapter.saved_llm));
    }

    #[test]
    fn only_new_remote_llm_endpoints_require_consent() {
        let mut settings = SaymoreSettings::default();
        let endpoint = "https://api.example.com/v1";

        assert!(llm_consent_required(&settings, endpoint));
        settings.llm.confirmed_base_url = endpoint.to_owned();
        assert!(!llm_consent_required(&settings, endpoint));
        assert!(!llm_consent_required(
            &settings,
            "http://localhost:11434/v1"
        ));
    }
}
