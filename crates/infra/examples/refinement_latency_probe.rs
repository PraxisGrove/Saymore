//! Compare fresh and reused production HTTP clients with an explicit test transcript.
//! Credentials and endpoint come only from SAYMORE_BENCH_LLM_* environment variables.
use std::{env, error::Error, io, sync::Arc, time::Instant};
use template_app::{
    ChatCompletionsLlmSettings, ChatCompletionsProfile, FinalTextProcessor, FinalTextRequest,
    RefinementMode, RefinementStrength,
};
use template_infra::ChatCompletionsLlmProvider;
use tokio_util::sync::CancellationToken;

fn provider() -> Result<FinalTextProcessor, Box<dyn Error>> {
    let profile = env::var("SAYMORE_BENCH_LLM_PROFILE").unwrap_or_else(|_| "portable".into());
    Ok(FinalTextProcessor::configured(Arc::new(
        ChatCompletionsLlmProvider::new(ChatCompletionsLlmSettings {
            base_url: env::var("SAYMORE_BENCH_LLM_URL")?,
            model: env::var("SAYMORE_BENCH_LLM_MODEL")?,
            api_key: env::var("SAYMORE_BENCH_LLM_KEY").unwrap_or_default(),
            custom_headers: Default::default(),
            profile: ChatCompletionsProfile::from_id(&profile)
                .ok_or_else(|| io::Error::other("invalid profile"))?,
        })?,
    )))
}

fn main() -> Result<(), Box<dyn Error>> {
    use tracing_subscriber::prelude::*;
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(io::stderr)
                .with_ansi(false)
                .with_filter(tracing_subscriber::filter::filter_fn(|metadata| {
                    metadata.target() == "saymore::diagnostics"
                })),
        )
        .init();
    let path = env::args()
        .nth(1)
        .ok_or_else(|| io::Error::other("expected UTF-8 test transcript file"))?;
    let text = std::fs::read_to_string(path)?;
    let runtime = tokio::runtime::Runtime::new()?;
    let reused = provider()?;
    for repeat in 0..6 {
        // Alternate order to reduce time-of-run bias.
        let modes = if repeat % 2 == 0 {
            ["fresh", "reused"]
        } else {
            ["reused", "fresh"]
        };
        for mode in modes {
            let start = Instant::now();
            let fresh;
            let processor = if mode == "fresh" {
                fresh = provider()?;
                &fresh
            } else {
                &reused
            };
            let mut request = FinalTextRequest::new(text.clone(), RefinementMode::Enabled);
            request.strength = RefinementStrength::Standard;
            let output = runtime.block_on(processor.process(request, CancellationToken::new()))?;
            println!(
                "{}",
                serde_json::json!({"client":mode,"repeat":repeat,
                "seconds":start.elapsed().as_secs_f64(),"status":format!("{:?}",output.refinement),
                "text":output.text})
            );
        }
    }
    Ok(())
}
