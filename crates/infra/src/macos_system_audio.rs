use std::{ffi::c_void, mem::size_of, ptr::NonNull};

use objc2_core_audio::{
    AudioObjectGetPropertyData, AudioObjectHasProperty, AudioObjectID, AudioObjectPropertyAddress,
    AudioObjectSetPropertyData, kAudioDevicePropertyMute, kAudioDevicePropertyVolumeScalar,
    kAudioHardwarePropertyDefaultOutputDevice, kAudioObjectPropertyElementMain,
    kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyScopeOutput, kAudioObjectSystemObject,
};
use template_app::{OutputAudioMuteSession, OutputAudioMuter, SystemAudioMuteError};

#[derive(Default)]
pub struct MacOsOutputAudioMuter;

impl OutputAudioMuter for MacOsOutputAudioMuter {
    fn begin_mute(&self) -> Result<Box<dyn OutputAudioMuteSession>, SystemAudioMuteError> {
        let device = default_output_device()?;
        if let Ok(session) = CoreAudioMuteSession::begin_with_volume(device) {
            return Ok(Box::new(session));
        }
        Ok(Box::new(CoreAudioMuteSession::begin_with_mute_property(
            device,
        )?))
    }
}

#[derive(Clone, Copy)]
enum ChangedProperty {
    Mute { original: u32 },
    Volume { original: f32 },
}

struct CoreAudioMuteSession {
    device: AudioObjectID,
    changed: Option<ChangedProperty>,
}

impl CoreAudioMuteSession {
    fn begin_with_mute_property(device: AudioObjectID) -> Result<Self, SystemAudioMuteError> {
        let address = output_property(kAudioDevicePropertyMute);
        if !has_property(device, &address) {
            return Err(unavailable("the output device has no mute property"));
        }
        let original: u32 = read_property(device, &address)?;
        if original == 0 {
            write_property(device, &address, 1_u32)?;
        }
        Ok(Self {
            device,
            changed: (original == 0).then_some(ChangedProperty::Mute { original }),
        })
    }

    fn begin_with_volume(device: AudioObjectID) -> Result<Self, SystemAudioMuteError> {
        let address = output_property(kAudioDevicePropertyVolumeScalar);
        if !has_property(device, &address) {
            return Err(unavailable(
                "the output device supports neither mute nor main volume",
            ));
        }
        let original: f32 = read_property(device, &address)?;
        if original > f32::EPSILON {
            write_property(device, &address, 0.0_f32)?;
        }
        Ok(Self {
            device,
            changed: (original > f32::EPSILON).then_some(ChangedProperty::Volume { original }),
        })
    }
}

impl OutputAudioMuteSession for CoreAudioMuteSession {
    fn restore(&mut self) -> Result<(), SystemAudioMuteError> {
        let Some(changed) = self.changed else {
            return Ok(());
        };
        match changed {
            ChangedProperty::Mute { original } => {
                let address = output_property(kAudioDevicePropertyMute);
                if read_property::<u32>(self.device, &address)? != 0 {
                    write_property(self.device, &address, original)?;
                }
            }
            ChangedProperty::Volume { original } => {
                let address = output_property(kAudioDevicePropertyVolumeScalar);
                if read_property::<f32>(self.device, &address)?.abs() <= f32::EPSILON {
                    write_property(self.device, &address, original)?;
                }
            }
        }
        self.changed = None;
        Ok(())
    }
}

impl Drop for CoreAudioMuteSession {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

fn default_output_device() -> Result<AudioObjectID, SystemAudioMuteError> {
    let address = AudioObjectPropertyAddress {
        mSelector: kAudioHardwarePropertyDefaultOutputDevice,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };
    read_property(kAudioObjectSystemObject as AudioObjectID, &address)
}

fn output_property(selector: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: kAudioObjectPropertyScopeOutput,
        mElement: kAudioObjectPropertyElementMain,
    }
}

fn has_property(object: AudioObjectID, address: &AudioObjectPropertyAddress) -> bool {
    unsafe { AudioObjectHasProperty(object, NonNull::from_ref(address)) }
}

fn read_property<T: Copy>(
    object: AudioObjectID,
    address: &AudioObjectPropertyAddress,
) -> Result<T, SystemAudioMuteError> {
    let mut value = std::mem::MaybeUninit::<T>::uninit();
    let mut size = size_of::<T>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            object,
            NonNull::from_ref(address),
            0,
            std::ptr::null(),
            NonNull::from_mut(&mut size),
            NonNull::new(value.as_mut_ptr().cast::<c_void>())
                .ok_or_else(|| unavailable("CoreAudio returned a null output buffer"))?,
        )
    };
    if status != 0 {
        return Err(status_error("read", status));
    }
    Ok(unsafe { value.assume_init() })
}

fn write_property<T>(
    object: AudioObjectID,
    address: &AudioObjectPropertyAddress,
    mut value: T,
) -> Result<(), SystemAudioMuteError> {
    let status = unsafe {
        AudioObjectSetPropertyData(
            object,
            NonNull::from_ref(address),
            0,
            std::ptr::null(),
            size_of::<T>() as u32,
            NonNull::from_mut(&mut value).cast::<c_void>(),
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(status_error("write", status))
    }
}

fn status_error(operation: &str, status: i32) -> SystemAudioMuteError {
    unavailable(format!("CoreAudio {operation} failed with status {status}"))
}

fn unavailable(reason: impl Into<String>) -> SystemAudioMuteError {
    SystemAudioMuteError::Unavailable(reason.into())
}
