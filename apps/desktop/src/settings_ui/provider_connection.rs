use template_app::{
    AsrProviderConfiguration, LlmProviderConfiguration, LlmProviderError, ProviderConnectionTester,
    SpeechRecognitionError,
};
use template_infra::ChatCompletionsLlmProvider;

use super::asr_configuration::recognition_test;

pub(super) struct DesktopProviderConnectionTester;

impl ProviderConnectionTester for DesktopProviderConnectionTester {
    fn test_asr(
        &self,
        candidate: &AsrProviderConfiguration,
    ) -> Result<String, SpeechRecognitionError> {
        recognition_test::run(candidate).result
    }

    fn test_llm(&self, candidate: &LlmProviderConfiguration) -> Result<(), LlmProviderError> {
        let provider = ChatCompletionsLlmProvider::new(candidate.settings().clone())?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| {
                LlmProviderError::Transport("LLM connection-test runtime is unavailable".to_owned())
            })?;
        runtime.block_on(provider.test_connection())
    }
}
