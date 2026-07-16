use std::sync::{Arc, atomic::AtomicBool};

use template_app::{FeedbackSound, FeedbackSoundPlayer};
use template_infra::MacOsFeedbackSoundPlayer;

pub(crate) fn play_feedback_sound(sound: FeedbackSound) {
    if let Err(error) = MacOsFeedbackSoundPlayer.play(sound) {
        eprintln!("failed to play feedback sound: {error}");
    }
}

pub(crate) fn initialize(enabled: bool) -> Arc<AtomicBool> {
    if let Err(error) = MacOsFeedbackSoundPlayer.preload() {
        tracing::warn!(event = "feedback.preload_failed", reason = %error);
    }
    Arc::new(AtomicBool::new(enabled))
}
