use std::sync::Arc;

use thiserror::Error;

/// Provider-independent vocabulary hints for one recognition session.
///
/// Implementations may apply provider-specific validation and limits before
/// sending these terms to a speech-recognition service.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpeechRecognitionHints {
    terms: Vec<String>,
}

impl SpeechRecognitionHints {
    pub fn from_terms(terms: Vec<String>) -> Self {
        Self { terms }
    }

    pub fn terms(&self) -> &[String] {
        &self.terms
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SpeechRecognitionError {
    #[error("speech recognition is not configured")]
    NotConfigured,
    #[error("speech recognition authentication failed")]
    Authentication,
    #[error("speech recognition quota is unavailable")]
    Quota,
    #[error("speech recognition transport failed: {0}")]
    Transport(String),
    #[error("speech recognition protocol failed: {0}")]
    Protocol(String),
    #[error("speech recognition timed out")]
    Timeout,
}

/// Owns one speech-recognition session receiving ordered audio chunks.
///
/// Chunked input does not imply incremental inference: adapters may process
/// audio during capture or buffer it until `finish`. Implementations must keep
/// partial text in memory and return exactly one final transcript after `finish`.
/// Calling `cancel` must prevent a later final result from being delivered.
pub trait StreamingRecognitionSession: Send {
    fn push_audio(&self, samples: Vec<i16>) -> Result<(), SpeechRecognitionError>;

    fn finish(self: Box<Self>) -> Result<String, SpeechRecognitionError>;

    fn cancel(self: Box<Self>);
}

/// Starts provider-specific ASR sessions with chunked audio input.
///
/// Implementations may connect in the background, but must accept audio chunks
/// immediately. Adapters that recognize during capture report genuine interim
/// transcripts through `on_partial`; adapters that decode only at `finish` leave
/// it unused. This shared port does not declare every model's supported modes,
/// and generating text tokens after capture is not live audio recognition.
pub trait StreamingSpeechRecognizer: Send + Sync {
    fn start(
        &self,
        hints: SpeechRecognitionHints,
        on_partial: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Result<Box<dyn StreamingRecognitionSession>, SpeechRecognitionError>;
}
