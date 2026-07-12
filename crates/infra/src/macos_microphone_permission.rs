use block2::RcBlock;
use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};
use template_app::{MicrophoneAuthorization, MicrophonePermissionProvider};

#[derive(Debug, Clone, Copy, Default)]
pub struct MacOsMicrophonePermission;

impl MicrophonePermissionProvider for MacOsMicrophonePermission {
    fn authorization(&self) -> MicrophoneAuthorization {
        microphone_authorization()
    }

    fn request_authorization(&self) -> MicrophoneAuthorization {
        if self.authorization() == MicrophoneAuthorization::NotDetermined
            && let Some(media_type) = unsafe { AVMediaTypeAudio }
        {
            let completion = RcBlock::new(|_granted| {});
            unsafe {
                AVCaptureDevice::requestAccessForMediaType_completionHandler(
                    media_type,
                    &completion,
                );
            }
        }
        self.authorization()
    }
}

fn microphone_authorization() -> MicrophoneAuthorization {
    let Some(media_type) = (unsafe { AVMediaTypeAudio }) else {
        return MicrophoneAuthorization::Denied;
    };
    let status = unsafe { AVCaptureDevice::authorizationStatusForMediaType(media_type) };
    match status {
        AVAuthorizationStatus::NotDetermined => MicrophoneAuthorization::NotDetermined,
        AVAuthorizationStatus::Authorized => MicrophoneAuthorization::Granted,
        AVAuthorizationStatus::Restricted => MicrophoneAuthorization::Restricted,
        AVAuthorizationStatus::Denied => MicrophoneAuthorization::Denied,
        _ => MicrophoneAuthorization::Denied,
    }
}
