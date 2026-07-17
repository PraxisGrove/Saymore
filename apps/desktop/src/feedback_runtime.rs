use std::sync::{Arc, atomic::AtomicBool};

use template_app::{FeedbackSound, FeedbackSoundPlayer};
#[cfg(target_os = "macos")]
use template_infra::MacOsFeedbackSoundPlayer;
#[cfg(target_os = "windows")]
use template_infra::WindowsFeedbackSoundPlayer;

pub(crate) fn play_feedback_sound(sound: FeedbackSound) {
    #[cfg(target_os = "macos")]
    if let Err(error) = MacOsFeedbackSoundPlayer.play(sound) {
        eprintln!("failed to play feedback sound: {error}");
    }
    #[cfg(target_os = "windows")]
    if let Err(error) = WindowsFeedbackSoundPlayer.play(sound) {
        eprintln!("failed to play feedback sound: {error}");
    }
}

pub(crate) fn initialize(enabled: bool) -> Arc<AtomicBool> {
    #[cfg(target_os = "macos")]
    if let Err(error) = MacOsFeedbackSoundPlayer.preload() {
        tracing::warn!(event = "feedback.preload_failed", reason = %error);
    }
    Arc::new(AtomicBool::new(enabled))
}
