use std::{sync::mpsc, thread};

use template_app::{OutputAudioMuteSession, OutputAudioMuter, SystemAudioMuteError};
use windows::{
    Win32::{
        Media::Audio::{
            Endpoints::IAudioEndpointVolume, IMMDeviceEnumerator, MMDeviceEnumerator, eMultimedia,
            eRender,
        },
        System::Com::{
            CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
        },
    },
    core::Result as WindowsResult,
};

#[derive(Default)]
pub struct WindowsOutputAudioMuter;

impl OutputAudioMuter for WindowsOutputAudioMuter {
    fn begin_mute(&self) -> Result<Box<dyn OutputAudioMuteSession>, SystemAudioMuteError> {
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let (restore_sender, restore_receiver) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("saymore-system-audio-mute".to_owned())
            .spawn(move || run_mute_session(ready_sender, restore_receiver))
            .map_err(|error| unavailable(error.to_string()))?;
        match ready_receiver.recv() {
            Ok(Ok(())) => Ok(Box::new(WindowsMuteSession {
                restore_sender: Some(restore_sender),
                worker: Some(worker),
            })),
            Ok(Err(error)) => {
                let _ = worker.join();
                Err(error)
            }
            Err(error) => {
                let _ = worker.join();
                Err(unavailable(error.to_string()))
            }
        }
    }
}

struct WindowsMuteSession {
    restore_sender: Option<mpsc::SyncSender<()>>,
    worker: Option<thread::JoinHandle<Result<(), SystemAudioMuteError>>>,
}

impl OutputAudioMuteSession for WindowsMuteSession {
    fn restore(&mut self) -> Result<(), SystemAudioMuteError> {
        if let Some(sender) = self.restore_sender.take() {
            let _ = sender.send(());
        }
        match self.worker.take() {
            Some(worker) => worker
                .join()
                .map_err(|_| unavailable("the system audio mute worker panicked"))?,
            None => Ok(()),
        }
    }
}

impl Drop for WindowsMuteSession {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

fn run_mute_session(
    ready: mpsc::SyncSender<Result<(), SystemAudioMuteError>>,
    restore: mpsc::Receiver<()>,
) -> Result<(), SystemAudioMuteError> {
    let (apartment, endpoint, original_volume) = match prepare_mute_session() {
        Ok(prepared) => prepared,
        Err(error) => {
            let _ = ready.send(Err(error.clone()));
            return Err(error);
        }
    };
    if ready.send(Ok(())).is_err() {
        restore_endpoint(&endpoint, original_volume)?;
        return Ok(());
    }
    let _ = restore.recv();
    restore_endpoint(&endpoint, original_volume)?;
    drop(apartment);
    Ok(())
}

fn prepare_mute_session() -> Result<(ComApartment, IAudioEndpointVolume, f32), SystemAudioMuteError>
{
    let apartment = ComApartment::initialize()?;
    let endpoint = default_endpoint_volume().map_err(windows_error)?;
    let original_volume =
        unsafe { endpoint.GetMasterVolumeLevelScalar() }.map_err(windows_error)?;
    if original_volume > f32::EPSILON {
        unsafe { endpoint.SetMasterVolumeLevelScalar(0.0, std::ptr::null()) }
            .map_err(windows_error)?;
    }
    Ok((apartment, endpoint, original_volume))
}

fn restore_endpoint(
    endpoint: &IAudioEndpointVolume,
    original_volume: f32,
) -> Result<(), SystemAudioMuteError> {
    if original_volume > f32::EPSILON
        && unsafe { endpoint.GetMasterVolumeLevelScalar() }.map_err(windows_error)? <= f32::EPSILON
    {
        unsafe { endpoint.SetMasterVolumeLevelScalar(original_volume, std::ptr::null()) }
            .map_err(windows_error)?;
    }
    Ok(())
}

fn default_endpoint_volume() -> WindowsResult<IAudioEndpointVolume> {
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let endpoint = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia)?;
        endpoint.Activate(CLSCTX_ALL, None)
    }
}

struct ComApartment;

impl ComApartment {
    fn initialize() -> Result<Self, SystemAudioMuteError> {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .ok()
            .map_err(windows_error)?;
        Ok(Self)
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

fn windows_error(error: windows::core::Error) -> SystemAudioMuteError {
    unavailable(error.to_string())
}

fn unavailable(reason: impl Into<String>) -> SystemAudioMuteError {
    SystemAudioMuteError::Unavailable(reason.into())
}
