use std::sync::Arc;

use thiserror::Error;

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

/// Owns one live speech-recognition connection.
///
/// Implementations must preserve audio order, keep partial text in memory, and
/// return exactly one final transcript after `finish`. Calling `cancel` must
/// prevent a later final result from being delivered.
pub trait StreamingRecognitionSession: Send {
    fn push_audio(&self, samples: Vec<i16>) -> Result<(), SpeechRecognitionError>;

    fn finish(self: Box<Self>) -> Result<String, SpeechRecognitionError>;

    fn cancel(self: Box<Self>);
}

/// Starts provider-specific streaming ASR sessions.
///
/// Implementations may connect in the background, but must accept audio chunks
/// immediately and report provider partial transcripts through `on_partial`.
pub trait StreamingSpeechRecognizer: Send + Sync {
    fn start(
        &self,
        on_partial: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Result<Box<dyn StreamingRecognitionSession>, SpeechRecognitionError>;
}
