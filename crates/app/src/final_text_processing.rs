use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use thiserror::Error;
use tokio::{sync::Mutex, time::Instant};
use tokio_util::sync::CancellationToken;

use crate::refinement_policy::accepts_refinement;
use crate::refinement_prompt::instructions_for;

const REFINEMENT_TIMEOUT: Duration = Duration::from_secs(8);
const FAILURE_PAUSE: Duration = Duration::from_secs(5 * 60);
const FAILURE_THRESHOLD: u8 = 3;
const MIN_CJK_REFINEMENT_UNITS: usize = 12;
const MIN_ENGLISH_REFINEMENT_WORDS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmRefinementRequest {
    pub instructions: String,
    pub transcript: String,
    pub language: Option<String>,
    pub relevant_terms: Vec<RefinementTerm>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefinementMode {
    Disabled,
    Enabled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinalTextRequest {
    pub transcript: String,
    pub refinement: RefinementMode,
    pub language: Option<String>,
    pub relevant_terms: Vec<RefinementTerm>,
}

impl FinalTextRequest {
    pub fn new(transcript: impl Into<String>, refinement: RefinementMode) -> Self {
        Self {
            transcript: transcript.into(),
            refinement,
            language: None,
            relevant_terms: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefinementTerm {
    pub canonical: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessedText {
    pub text: String,
    pub refinement: RefinementStatus,
}

/// Captures both the production-safe result and the raw provider candidate.
///
/// Evaluation callers may inspect `provider_output`; production delivery must
/// continue to use `processed` so rejected model output is never delivered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefinementEvaluation {
    pub processed: ProcessedText,
    pub provider_output: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefinementEvaluationMode {
    ProductionPolicy,
    ForceProvider,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefinementStatus {
    Disabled,
    Skipped(RefinementSkipReason),
    Completed,
    FellBack(RefinementFallbackReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefinementSkipReason {
    ShortTranscript,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefinementFallbackReason {
    NotConfigured,
    Authentication,
    InvalidConfiguration,
    ModelUnavailable,
    Quota,
    Transport,
    Protocol,
    Timeout,
    TemporarilyUnavailable,
    OutputRejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LlmProviderError {
    #[error("LLM authentication failed")]
    Authentication,
    #[error("the LLM configuration is invalid")]
    InvalidConfiguration,
    #[error("the configured LLM model is unavailable")]
    ModelUnavailable,
    #[error("LLM quota is unavailable")]
    Quota,
    #[error("LLM transport failed: {0}")]
    Transport(String),
    #[error("LLM protocol failed: {0}")]
    Protocol(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FinalTextProcessingError {
    #[error("final text processing was cancelled")]
    Cancelled,
}

/// Produces one refined transcript through a configured LLM provider.
///
/// Implementations must issue at most one provider request and must be safe to
/// cancel by dropping the returned future. They must not persist or log request
/// or response content.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn refine(&self, request: LlmRefinementRequest) -> Result<String, LlmProviderError>;
}

pub struct FinalTextProcessor {
    provider: Option<Arc<dyn LlmProvider>>,
    circuit: Mutex<RefinementCircuit>,
}

impl FinalTextProcessor {
    pub fn configured(provider: Arc<dyn LlmProvider>) -> Self {
        Self {
            provider: Some(provider),
            circuit: Mutex::new(RefinementCircuit::default()),
        }
    }

    pub fn unconfigured() -> Self {
        Self {
            provider: None,
            circuit: Mutex::new(RefinementCircuit::default()),
        }
    }

    pub async fn process(
        &self,
        request: FinalTextRequest,
        cancellation: CancellationToken,
    ) -> Result<ProcessedText, FinalTextProcessingError> {
        self.process_with_attempt_observer(request, cancellation, || {})
            .await
    }

    pub async fn process_with_attempt_observer<F>(
        &self,
        request: FinalTextRequest,
        cancellation: CancellationToken,
        on_provider_attempt: F,
    ) -> Result<ProcessedText, FinalTextProcessingError>
    where
        F: FnOnce(),
    {
        self.execute(
            request,
            cancellation,
            RefinementEvaluationMode::ProductionPolicy,
            on_provider_attempt,
        )
        .await
        .map(|evaluation| evaluation.processed)
    }

    pub async fn evaluate(
        &self,
        request: FinalTextRequest,
        cancellation: CancellationToken,
        mode: RefinementEvaluationMode,
    ) -> Result<RefinementEvaluation, FinalTextProcessingError> {
        self.execute(request, cancellation, mode, || {}).await
    }

    async fn execute<F>(
        &self,
        request: FinalTextRequest,
        cancellation: CancellationToken,
        mode: RefinementEvaluationMode,
        on_provider_attempt: F,
    ) -> Result<RefinementEvaluation, FinalTextProcessingError>
    where
        F: FnOnce(),
    {
        reject_cancelled(&cancellation)?;
        let text = clean_transcript(&request.transcript);
        let RefinementMode::Enabled = request.refinement else {
            reject_cancelled(&cancellation)?;
            return Ok(evaluation_without_provider(ProcessedText {
                text,
                refinement: RefinementStatus::Disabled,
            }));
        };
        if mode == RefinementEvaluationMode::ProductionPolicy
            && !refinement_needed(&text, &RefinementMode::Enabled)
        {
            reject_cancelled(&cancellation)?;
            return Ok(evaluation_without_provider(ProcessedText {
                text,
                refinement: RefinementStatus::Skipped(RefinementSkipReason::ShortTranscript),
            }));
        }
        let Some(provider) = &self.provider else {
            reject_cancelled(&cancellation)?;
            return Ok(evaluation_without_provider(ProcessedText {
                text,
                refinement: RefinementStatus::FellBack(RefinementFallbackReason::NotConfigured),
            }));
        };
        if let Some(reason) = self.circuit.lock().await.bypass_reason(Instant::now()) {
            reject_cancelled(&cancellation)?;
            return Ok(evaluation_without_provider(ProcessedText {
                text,
                refinement: RefinementStatus::FellBack(reason),
            }));
        }
        let fallback_text = text.clone();
        let relevant_terms = request.relevant_terms;
        let provider_request = LlmRefinementRequest {
            instructions: instructions_for(&text),
            transcript: text,
            language: request.language,
            relevant_terms: relevant_terms.clone(),
        };
        on_provider_attempt();
        let refined = tokio::select! {
            () = cancellation.cancelled() => return Err(FinalTextProcessingError::Cancelled),
            result = tokio::time::timeout(
                REFINEMENT_TIMEOUT,
                provider.refine(provider_request),
            ) => result,
        };
        self.complete_provider_attempt(refined, fallback_text, &relevant_terms, &cancellation)
            .await
    }

    async fn complete_provider_attempt(
        &self,
        refined: Result<Result<String, LlmProviderError>, tokio::time::error::Elapsed>,
        fallback_text: String,
        relevant_terms: &[RefinementTerm],
        cancellation: &CancellationToken,
    ) -> Result<RefinementEvaluation, FinalTextProcessingError> {
        match refined {
            Ok(Ok(text)) if accepts_refinement(&fallback_text, &text, relevant_terms) => {
                reject_cancelled(cancellation)?;
                self.circuit.lock().await.record_success();
                let provider_output = text.trim().to_owned();
                Ok(RefinementEvaluation {
                    processed: ProcessedText {
                        text: provider_output.clone(),
                        refinement: RefinementStatus::Completed,
                    },
                    provider_output: Some(provider_output),
                })
            }
            Ok(Ok(text)) => {
                reject_cancelled(cancellation)?;
                Ok(RefinementEvaluation {
                    processed: ProcessedText {
                        text: fallback_text,
                        refinement: RefinementStatus::FellBack(
                            RefinementFallbackReason::OutputRejected,
                        ),
                    },
                    provider_output: Some(text.trim().to_owned()),
                })
            }
            Ok(Err(error)) => {
                reject_cancelled(cancellation)?;
                let reason = fallback_reason(&error);
                self.circuit
                    .lock()
                    .await
                    .record_failure(&error, Instant::now());
                Ok(evaluation_without_provider(ProcessedText {
                    text: fallback_text,
                    refinement: RefinementStatus::FellBack(reason),
                }))
            }
            Err(_) => {
                reject_cancelled(cancellation)?;
                self.circuit.lock().await.record_timeout(Instant::now());
                Ok(evaluation_without_provider(ProcessedText {
                    text: fallback_text,
                    refinement: RefinementStatus::FellBack(RefinementFallbackReason::Timeout),
                }))
            }
        }
    }
}

fn evaluation_without_provider(processed: ProcessedText) -> RefinementEvaluation {
    RefinementEvaluation {
        processed,
        provider_output: None,
    }
}

pub fn refinement_needed(transcript: &str, mode: &RefinementMode) -> bool {
    if *mode == RefinementMode::Disabled {
        return false;
    }
    let mut cjk_units = 0usize;
    let mut non_cjk_words = 0usize;
    let mut inside_non_cjk_word = false;
    let mut characters = transcript.chars().peekable();
    while let Some(character) = characters.next() {
        if is_cjk(character) {
            cjk_units = cjk_units.saturating_add(1);
            inside_non_cjk_word = false;
        } else if character.is_alphanumeric() {
            if !inside_non_cjk_word {
                non_cjk_words = non_cjk_words.saturating_add(1);
                inside_non_cjk_word = true;
            }
        } else if is_internal_word_joiner(character)
            && inside_non_cjk_word
            && characters.peek().is_some_and(|next| next.is_alphanumeric())
        {
            continue;
        } else {
            inside_non_cjk_word = false;
        }
    }
    if cjk_units == 0 {
        non_cjk_words >= MIN_ENGLISH_REFINEMENT_WORDS
    } else {
        cjk_units.saturating_add(non_cjk_words.saturating_mul(2)) >= MIN_CJK_REFINEMENT_UNITS
    }
}

fn is_internal_word_joiner(character: char) -> bool {
    matches!(character, '\'' | '\u{2019}' | '-')
}

fn is_cjk(character: char) -> bool {
    matches!(
        character,
        '\u{3400}'..='\u{4dbf}'
            | '\u{4e00}'..='\u{9fff}'
            | '\u{f900}'..='\u{faff}'
            | '\u{3040}'..='\u{30ff}'
            | '\u{ac00}'..='\u{d7af}'
    )
}

fn reject_cancelled(cancellation: &CancellationToken) -> Result<(), FinalTextProcessingError> {
    if cancellation.is_cancelled() {
        Err(FinalTextProcessingError::Cancelled)
    } else {
        Ok(())
    }
}

#[derive(Default)]
struct RefinementCircuit {
    consecutive_failures: u8,
    availability: CircuitAvailability,
}

#[derive(Default)]
enum CircuitAvailability {
    #[default]
    Available,
    PausedUntil(Instant),
    Blocked(RefinementFallbackReason),
}

impl RefinementCircuit {
    fn bypass_reason(&mut self, now: Instant) -> Option<RefinementFallbackReason> {
        match &self.availability {
            CircuitAvailability::Available => None,
            CircuitAvailability::PausedUntil(until) if now < *until => {
                Some(RefinementFallbackReason::TemporarilyUnavailable)
            }
            CircuitAvailability::PausedUntil(_) => {
                self.consecutive_failures = 0;
                self.availability = CircuitAvailability::Available;
                None
            }
            CircuitAvailability::Blocked(reason) => Some(reason.clone()),
        }
    }

    fn record_success(&mut self) {
        self.consecutive_failures = 0;
        self.availability = CircuitAvailability::Available;
    }

    fn record_failure(&mut self, error: &LlmProviderError, now: Instant) {
        match error {
            LlmProviderError::Authentication => {
                self.availability =
                    CircuitAvailability::Blocked(RefinementFallbackReason::Authentication);
            }
            LlmProviderError::InvalidConfiguration => {
                self.availability =
                    CircuitAvailability::Blocked(RefinementFallbackReason::InvalidConfiguration);
            }
            LlmProviderError::ModelUnavailable => {
                self.availability =
                    CircuitAvailability::Blocked(RefinementFallbackReason::ModelUnavailable);
            }
            LlmProviderError::Quota
            | LlmProviderError::Transport(_)
            | LlmProviderError::Protocol(_) => self.record_transient_failure(now),
        }
    }

    fn record_timeout(&mut self, now: Instant) {
        self.record_transient_failure(now);
    }

    fn record_transient_failure(&mut self, now: Instant) {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        if self.consecutive_failures >= FAILURE_THRESHOLD {
            self.availability = CircuitAvailability::PausedUntil(now + FAILURE_PAUSE);
        }
    }
}

fn fallback_reason(error: &LlmProviderError) -> RefinementFallbackReason {
    match error {
        LlmProviderError::Authentication => RefinementFallbackReason::Authentication,
        LlmProviderError::InvalidConfiguration => RefinementFallbackReason::InvalidConfiguration,
        LlmProviderError::ModelUnavailable => RefinementFallbackReason::ModelUnavailable,
        LlmProviderError::Quota => RefinementFallbackReason::Quota,
        LlmProviderError::Transport(_) => RefinementFallbackReason::Transport,
        LlmProviderError::Protocol(_) => RefinementFallbackReason::Protocol,
    }
}

fn clean_transcript(transcript: &str) -> String {
    transcript
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}
