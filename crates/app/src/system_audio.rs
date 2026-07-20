use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SystemAudioMuteError {
    #[error("system audio muting is unavailable: {0}")]
    Unavailable(String),
}

/// Restores the output-audio state captured when a mute session began.
///
/// Implementations must be idempotent and must not overwrite a volume or mute
/// change made by the user while the session was active.
pub trait OutputAudioMuteSession: Send {
    fn restore(&mut self) -> Result<(), SystemAudioMuteError>;
}

/// Starts a scoped mute of the operating system's current output device.
///
/// Implementations capture only the state they change. Dropping the returned
/// session must restore that state on a best-effort basis.
pub trait OutputAudioMuter: Send + Sync {
    fn begin_mute(&self) -> Result<Box<dyn OutputAudioMuteSession>, SystemAudioMuteError>;
}
