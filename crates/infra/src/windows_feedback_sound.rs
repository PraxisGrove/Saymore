use template_app::{FeedbackSound, FeedbackSoundError, FeedbackSoundPlayer};
use windows::{
    Win32::Media::Audio::{PlaySoundA, SND_ASYNC, SND_MEMORY, SND_NODEFAULT},
    core::PCSTR,
};

const START_SOUND: &[u8] = include_bytes!("../assets/sounds/dictation-start.wav");
const FINISH_SOUND: &[u8] = include_bytes!("../assets/sounds/dictation-finish.wav");

#[derive(Debug, Clone, Copy, Default)]
pub struct WindowsFeedbackSoundPlayer;

impl FeedbackSoundPlayer for WindowsFeedbackSoundPlayer {
    fn play(&self, sound: FeedbackSound) -> Result<(), FeedbackSoundError> {
        let bytes = match sound {
            FeedbackSound::Start => START_SOUND,
            FeedbackSound::Finish => FINISH_SOUND,
        };
        // SAFETY: SND_MEMORY treats the static byte slice as a WAV image and SND_ASYNC
        // may retain it after this call; both embedded slices live for the process lifetime.
        let played = unsafe {
            PlaySoundA(
                PCSTR(bytes.as_ptr()),
                None,
                SND_ASYNC | SND_MEMORY | SND_NODEFAULT,
            )
        };
        if played.as_bool() {
            Ok(())
        } else {
            Err(FeedbackSoundError::PlaybackFailed)
        }
    }
}
