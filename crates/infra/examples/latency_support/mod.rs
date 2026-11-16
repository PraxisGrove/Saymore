//! Explicit benchmark-only provider configuration; never reads user settings.
pub mod qwen;
pub mod scheduling;

use std::{env, error::Error, io, sync::Arc, time::Instant};
use template_app::{
    ChatCompletionsLlmSettings, ChatCompletionsProfile, FinalTextProcessor, FinalTextRequest,
    RefinementMode, RefinementStrength,
};
use template_infra::ChatCompletionsLlmProvider;
use tokio_util::sync::CancellationToken;

pub fn process(text: String, strength: &str) -> Result<serde_json::Value, Box<dyn Error>> {
    let start = Instant::now();
    if strength == "disabled" {
        return Ok(serde_json::json!({"text": text, "status": "Disabled", "seconds": 0.0}));
    }
    let strength = RefinementStrength::from_storage_value(strength)
        .ok_or_else(|| io::Error::other("invalid refinement strength"))?;
    let profile = env::var("SAYMORE_BENCH_LLM_PROFILE").unwrap_or_else(|_| "portable".into());
    let provider = ChatCompletionsLlmProvider::new(ChatCompletionsLlmSettings {
        base_url: env::var("SAYMORE_BENCH_LLM_URL")?,
        model: env::var("SAYMORE_BENCH_LLM_MODEL")?,
        api_key: env::var("SAYMORE_BENCH_LLM_KEY").unwrap_or_default(),
        custom_headers: Default::default(),
        profile: ChatCompletionsProfile::from_id(&profile)
            .ok_or_else(|| io::Error::other("invalid LLM profile"))?,
    })?;
    let processor = FinalTextProcessor::configured(Arc::new(provider));
    let mut request = FinalTextRequest::new(text, RefinementMode::Enabled);
    request.strength = strength;
    let runtime = tokio::runtime::Runtime::new()?;
    let output = runtime.block_on(processor.process(request, CancellationToken::new()))?;
    Ok(
        serde_json::json!({"text": output.text, "status": format!("{:?}", output.refinement),
        "seconds": start.elapsed().as_secs_f64()}),
    )
}
