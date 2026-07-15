use std::cell::RefCell;

use objc2::{AnyThread, rc::Retained};
use objc2_app_kit::NSSound;
use objc2_foundation::NSData;
use template_app::{FeedbackSound, FeedbackSoundError, FeedbackSoundPlayer};

const START_SOUND: &[u8] = include_bytes!("../assets/sounds/dictation-start.wav");
const FINISH_SOUND: &[u8] = include_bytes!("../assets/sounds/dictation-finish.wav");

thread_local! {
    static CACHED_SOUNDS: RefCell<Option<CachedSounds>> = const { RefCell::new(None) };
}

struct CachedSounds {
    start: Retained<NSSound>,
    finish: Retained<NSSound>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MacOsFeedbackSoundPlayer;

impl MacOsFeedbackSoundPlayer {
    pub fn preload(&self) -> Result<(), FeedbackSoundError> {
        CACHED_SOUNDS.with(|cached| {
            let mut cached = cached.borrow_mut();
            if cached.is_none() {
                *cached = Some(CachedSounds::load()?);
            }
            Ok(())
        })
    }
}

impl CachedSounds {
    fn load() -> Result<Self, FeedbackSoundError> {
        Ok(Self {
            start: sound_from_bytes(START_SOUND)?,
            finish: sound_from_bytes(FINISH_SOUND)?,
        })
    }

    fn get(&self, sound: FeedbackSound) -> &NSSound {
        match sound {
            FeedbackSound::Start => &self.start,
            FeedbackSound::Finish => &self.finish,
        }
    }
}

impl FeedbackSoundPlayer for MacOsFeedbackSoundPlayer {
    fn play(&self, sound: FeedbackSound) -> Result<(), FeedbackSoundError> {
        self.preload()?;
        CACHED_SOUNDS.with(|cached| {
            let cached = cached.borrow();
            let cached = cached.as_ref().ok_or(FeedbackSoundError::Unavailable)?;
            if cached.get(sound).play() {
                Ok(())
            } else {
                Err(FeedbackSoundError::PlaybackFailed)
            }
        })
    }
}

#[cfg(test)]
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

    #[test]
    fn preloads_and_reuses_both_feedback_sounds() {
        let player = MacOsFeedbackSoundPlayer;
        assert!(player.preload().is_ok());
        CACHED_SOUNDS.with(|cached| assert!(cached.borrow().is_some()));
    }
}
