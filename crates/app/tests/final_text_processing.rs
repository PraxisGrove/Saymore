use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use template_app::{
    FinalTextProcessor, FinalTextRequest, LlmProvider, LlmProviderError, LlmRefinementRequest,
    ProcessedText, RefinementFallbackReason, RefinementMode, RefinementStatus,
};
use tokio_util::sync::CancellationToken;

struct CountingProvider {
    calls: AtomicUsize,
    result: Result<String, LlmProviderError>,
}

#[async_trait]
impl LlmProvider for CountingProvider {
    async fn refine(&self, _request: LlmRefinementRequest) -> Result<String, LlmProviderError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.result.clone()
    }
}

struct PendingProvider;

#[async_trait]
impl LlmProvider for PendingProvider {
    async fn refine(&self, _request: LlmRefinementRequest) -> Result<String, LlmProviderError> {
        std::future::pending().await
    }
}

fn enabled_request(transcript: &str) -> FinalTextRequest {
    FinalTextRequest::new(transcript, RefinementMode::Enabled)
}

#[tokio::test]
async fn disabled_refinement_returns_safely_cleaned_text_without_calling_provider() {
    let provider = Arc::new(CountingProvider {
        calls: AtomicUsize::new(0),
        result: Ok("unused".to_owned()),
    });
    let processor = FinalTextProcessor::configured(provider.clone());

    let result = processor
        .process(
            FinalTextRequest::new("  hello   world  \n  next line  ", RefinementMode::Disabled),
            CancellationToken::new(),
        )
        .await;

    assert_eq!(
        Ok(ProcessedText {
            text: "hello world\nnext line".to_owned(),
            refinement: RefinementStatus::Disabled,
        }),
        result
    );
    assert_eq!(0, provider.calls.load(Ordering::Relaxed));
}

#[tokio::test]
async fn enabled_refinement_returns_one_provider_result() {
    let provider = Arc::new(CountingProvider {
        calls: AtomicUsize::new(0),
        result: Ok("Refined text.".to_owned()),
    });
    let processor = FinalTextProcessor::configured(provider.clone());

    let result = processor
        .process(
            FinalTextRequest::new("  raw   text  ", RefinementMode::Enabled),
            CancellationToken::new(),
        )
        .await;

    assert_eq!(
        Ok(ProcessedText {
            text: "Refined text.".to_owned(),
            refinement: RefinementStatus::Completed,
        }),
        result
    );
    assert_eq!(1, provider.calls.load(Ordering::Relaxed));
}

#[tokio::test]
async fn provider_failure_falls_back_to_safely_cleaned_text() {
    let provider = Arc::new(CountingProvider {
        calls: AtomicUsize::new(0),
        result: Err(LlmProviderError::Transport("offline".to_owned())),
    });
    let processor = FinalTextProcessor::configured(provider);

    let result = processor
        .process(
            FinalTextRequest::new("  keep   this  ", RefinementMode::Enabled),
            CancellationToken::new(),
        )
        .await;

    assert_eq!(
        Ok(ProcessedText {
            text: "keep this".to_owned(),
            refinement: RefinementStatus::FellBack(RefinementFallbackReason::Transport),
        }),
        result
    );
}

#[tokio::test(start_paused = true)]
async fn provider_request_times_out_after_eight_seconds() {
    let processor = FinalTextProcessor::configured(Arc::new(PendingProvider));

    let result = processor
        .process(enabled_request("keep this"), CancellationToken::new())
        .await;

    assert_eq!(
        Ok(ProcessedText {
            text: "keep this".to_owned(),
            refinement: RefinementStatus::FellBack(RefinementFallbackReason::Timeout),
        }),
        result
    );
}

#[tokio::test]
async fn cancellation_prevents_a_late_refinement_result() {
    let processor = Arc::new(FinalTextProcessor::configured(Arc::new(PendingProvider)));
    let cancellation = CancellationToken::new();
    let task_processor = Arc::clone(&processor);
    let task_cancellation = cancellation.clone();
    let task = tokio::spawn(async move {
        task_processor
            .process(enabled_request("discard this"), task_cancellation)
            .await
    });
    tokio::task::yield_now().await;

    cancellation.cancel();

    assert!(matches!(
        task.await,
        Ok(Err(template_app::FinalTextProcessingError::Cancelled))
    ));
}

#[tokio::test]
async fn cancellation_also_stops_disabled_and_unconfigured_paths() {
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let disabled = FinalTextProcessor::unconfigured()
        .process(
            FinalTextRequest::new("discard this", RefinementMode::Disabled),
            cancellation.clone(),
        )
        .await;
    let unconfigured = FinalTextProcessor::unconfigured()
        .process(enabled_request("discard this"), cancellation)
        .await;

    assert_eq!(
        Err(template_app::FinalTextProcessingError::Cancelled),
        disabled
    );
    assert_eq!(
        Err(template_app::FinalTextProcessingError::Cancelled),
        unconfigured
    );
}

#[tokio::test(start_paused = true)]
async fn three_transient_failures_pause_calls_for_five_minutes() {
    let provider = Arc::new(CountingProvider {
        calls: AtomicUsize::new(0),
        result: Err(LlmProviderError::Transport("offline".to_owned())),
    });
    let processor = FinalTextProcessor::configured(provider.clone());
    let cancellation = CancellationToken::new();

    for _ in 0..3 {
        let _ = processor
            .process(enabled_request("keep this"), cancellation.clone())
            .await;
    }
    let paused = processor
        .process(enabled_request("keep this"), cancellation.clone())
        .await;

    assert_eq!(
        Ok(ProcessedText {
            text: "keep this".to_owned(),
            refinement: RefinementStatus::FellBack(
                RefinementFallbackReason::TemporarilyUnavailable,
            ),
        }),
        paused
    );
    assert_eq!(3, provider.calls.load(Ordering::Relaxed));

    tokio::time::advance(std::time::Duration::from_secs(300)).await;
    let _ = processor
        .process(enabled_request("keep this"), cancellation)
        .await;
    assert_eq!(4, provider.calls.load(Ordering::Relaxed));
}
