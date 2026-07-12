use objc2_app_kit::NSSound;
use objc2_foundation::NSString;
use template_app::{FeedbackSound, FeedbackSoundError, FeedbackSoundPlayer};

#[derive(Debug, Clone, Copy, Default)]
pub struct MacOsFeedbackSoundPlayer;

impl FeedbackSoundPlayer for MacOsFeedbackSoundPlayer {
    fn play(&self, sound: FeedbackSound) -> Result<(), FeedbackSoundError> {
        let name = NSString::from_str(match sound {
            FeedbackSound::Start => "Tink",
            FeedbackSound::Finish => "Pop",
            FeedbackSound::Cancel => "Purr",
            FeedbackSound::Failure => "Basso",
        });
        let sound = NSSound::soundNamed(&name).ok_or(FeedbackSoundError::Unavailable)?;
        if sound.play() {
            Ok(())
        } else {
            Err(FeedbackSoundError::PlaybackFailed)
        }
    }
}
