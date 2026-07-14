use std::cell::RefCell;

use objc2::{AnyThread, rc::Retained};
use objc2_app_kit::NSSound;
use objc2_foundation::NSData;
use template_app::{FeedbackSound, FeedbackSoundError, FeedbackSoundPlayer};

const START_SOUND: &[u8] = include_bytes!("../assets/sounds/dictation-start.wav");
const FINISH_SOUND: &[u8] = include_bytes!("../assets/sounds/dictation-finish.wav");

thread_local! {
    static ACTIVE_SOUND: RefCell<Option<Retained<NSSound>>> = const { RefCell::new(None) };
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MacOsFeedbackSoundPlayer;

impl FeedbackSoundPlayer for MacOsFeedbackSoundPlayer {
    fn play(&self, sound: FeedbackSound) -> Result<(), FeedbackSoundError> {
        let sound = sound_from_bytes(bundled_sound_bytes(sound))?;
        if sound.play() {
            ACTIVE_SOUND.with(|active| active.replace(Some(sound)));
            Ok(())
        } else {
            Err(FeedbackSoundError::PlaybackFailed)
        }
    }
}

fn bundled_sound_bytes(sound: FeedbackSound) -> &'static [u8] {
    match sound {
        FeedbackSound::Start => START_SOUND,
        FeedbackSound::Finish => FINISH_SOUND,
    }
}

fn sound_from_bytes(bytes: &[u8]) -> Result<Retained<NSSound>, FeedbackSoundError> {
    // SAFETY: `bytes` remains valid for the duration of the copying NSData constructor.
    let data = unsafe { NSData::dataWithBytes_length(bytes.as_ptr().cast(), bytes.len()) };
    NSSound::initWithData(NSSound::alloc(), &data).ok_or(FeedbackSoundError::Unavailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_sounds_are_loadable_by_macos() {
        for sound in [FeedbackSound::Start, FeedbackSound::Finish] {
            assert!(sound_from_bytes(bundled_sound_bytes(sound)).is_ok());
        }
    }
}
