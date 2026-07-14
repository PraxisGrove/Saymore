use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedbackSound {
    Start,
    Finish,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FeedbackSoundError {
    #[error("the requested feedback sound is unavailable")]
    Unavailable,
    #[error("the feedback sound could not be played")]
    PlaybackFailed,
}

/// Plays short, non-blocking cues for dictation lifecycle transitions.
///
/// Implementations must not hold the microphone stream or block the UI event
/// loop while a cue is playing.
pub trait FeedbackSoundPlayer {
    fn play(&self, sound: FeedbackSound) -> Result<(), FeedbackSoundError>;
}
