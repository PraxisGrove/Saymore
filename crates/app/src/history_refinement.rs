use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::{
    DictionaryStore, FinalTextProcessingError, FinalTextProcessor, FinalTextRequest,
    HistoryRefinementUpdate, HistoryStore, RefinementMode, RefinementStatus, RefinementStrength,
    StorageError, dictation_completion::inferred_transcript_language,
    dictionary_terms_for_refinement, normalize_standard_spellings,
};

/// The user's request to polish one saved dictation again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryRefinementRetry {
    pub history_id: String,
    pub transcript: String,
    pub strength: RefinementStrength,
    pub llm_provider_id: Option<String>,
    pub llm_model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum HistoryRefinementRetryError {
    #[error("the saved dictation has no recognized text to polish")]
    EmptyTranscript,
    #[error("text polishing did not complete: {0:?}")]
    NotRefined(RefinementStatus),
    #[error("the polished text could not be saved: {0}")]
    Storage(#[from] StorageError),
    #[error("text polishing was cancelled")]
    Cancelled,
}

/// Polishes a saved dictation again from its recognized text and saves the result.
///
/// Returns the delivered-ready polished text. History is changed only when the
/// provider output passed the same validation as a live dictation.
pub async fn retry_history_refinement(
    processor: &FinalTextProcessor,
    dictionary: &dyn DictionaryStore,
    history: &dyn HistoryStore,
    retry: HistoryRefinementRetry,
    cancellation: CancellationToken,
) -> Result<String, HistoryRefinementRetryError> {
    let transcript = retry.transcript.trim();
    if transcript.is_empty() {
        return Err(HistoryRefinementRetryError::EmptyTranscript);
    }
    let terms = dictionary_terms_for_refinement(dictionary).unwrap_or_default();
    let mut request = FinalTextRequest::new(transcript, RefinementMode::Enabled);
    request.strength = retry.strength;
    request.language = Some(inferred_transcript_language(transcript).to_owned());
    request.relevant_terms = terms.clone();
    let processed = processor
        .retry(request, cancellation)
        .await
        .map_err(|FinalTextProcessingError::Cancelled| HistoryRefinementRetryError::Cancelled)?;
    if processed.refinement != RefinementStatus::Completed {
        return Err(HistoryRefinementRetryError::NotRefined(
            processed.refinement,
        ));
    }
    let refined_text = normalize_standard_spellings(&processed.text, &terms);
    history.record_history_refinement(
        &retry.history_id,
        HistoryRefinementUpdate {
            refined_text: refined_text.clone(),
            llm_provider_id: retry.llm_provider_id,
            llm_model: retry.llm_model,
        },
    )?;
    Ok(refined_text)
}
