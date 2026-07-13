#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{error::Error, process::ExitCode};
#[cfg(target_os = "macos")]
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[cfg(target_os = "macos")]
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::{ComponentHandle, SharedString};
#[cfg(target_os = "macos")]
use slint::{Timer, TimerMode};
#[cfg(target_os = "macos")]
use template_app::{
    AudioRecorder, CancelledRecordingStore, MicrophonePermissionProvider, PcmChunk, RecordingError,
    SpeechRecognitionError,
};
#[cfg(target_os = "macos")]
use template_app::{FeedbackSound, FeedbackSoundPlayer, TextDeliverer};
#[cfg(target_os = "macos")]
use template_infra::{
    DictationShortcutAction, JsonSettingsStore, MacOsAudioRecorder, MacOsFeedbackSoundPlayer,
    MacOsMicrophonePermission, MacOsShortcutMonitor, MacOsTextDeliverer, configure_overlay_window,
};

// Slint-generated code contains framework-internal unwraps and panics. Keep the
// exception scoped to generated output; handwritten production code stays strict.
#[allow(
    clippy::panic,
    clippy::todo,
    clippy::unimplemented,
    clippy::unwrap_used
)]
mod ui {
    slint::include_modules!();
}

use ui::{AppWindow, RecordingOverlay, ResultOverlay};

#[cfg(target_os = "macos")]
mod asr_runtime;
#[cfg(target_os = "macos")]
mod delivery_runtime;
#[cfg(target_os = "macos")]
mod main_window;
#[cfg(target_os = "macos")]
mod recording_metrics;
#[cfg(target_os = "macos")]
mod settings_ui;
#[cfg(target_os = "macos")]
mod ui_status;

#[cfg(target_os = "macos")]
use asr_runtime::{AsrSessionController, normalize_transcript};
#[cfg(target_os = "macos")]
use ui_status::*;

#[cfg(target_os = "macos")]
const AUTHORIZATION_POLL_INTERVAL: Duration = Duration::from_secs(1);
#[cfg(target_os = "macos")]
const CANCEL_UNDO_WINDOW: Duration = Duration::from_secs(2);

#[cfg(target_os = "macos")]
enum FinishError {
    Recording(RecordingError),
    Recognition(SpeechRecognitionError),
}

#[cfg(target_os = "macos")]
#[derive(Clone)]
struct DictationOverlays {
    status: slint::Weak<RecordingOverlay>,
    result: slint::Weak<ResultOverlay>,
}

#[cfg(target_os = "macos")]
impl DictationOverlays {
    fn new(status: &RecordingOverlay, result: &ResultOverlay) -> Self {
        Self {
            status: status.as_weak(),
            result: result.as_weak(),
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("failed to run Saymore: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(target_os = "macos")]
fn run() -> Result<(), Box<dyn Error>> {
    let settings_store = Arc::new(JsonSettingsStore::for_current_user()?);
    settings_store.ensure_exists()?;
    let ui = AppWindow::new()?;
    let overlay = RecordingOverlay::new()?;
    let result_overlay = ResultOverlay::new()?;
    let deliverer = MacOsTextDeliverer;
    let microphone = MacOsMicrophonePermission;
    let recorder = Arc::new(Mutex::new(MacOsAudioRecorder::default()));
    let recording_active = Arc::new(AtomicBool::new(false));
    let asr = Arc::new(AsrSessionController::new(Arc::clone(&settings_store)));
    let cancelled = Arc::new(Mutex::new(CancelledRecordingStore::new(CANCEL_UNDO_WINDOW)));
    update_authorizations(&ui, deliverer.authorization(), microphone.authorization());
    settings_ui::wire(&ui, Arc::clone(&settings_store));

    let request_accessibility_ui = ui.as_weak();
    ui.on_request_authorization(move || {
        if let Some(ui) = request_accessibility_ui.upgrade() {
            update_accessibility_authorization(&ui, deliverer.request_authorization());
        }
    });

    let request_microphone_ui = ui.as_weak();
    ui.on_request_microphone_authorization(move || {
        if let Some(ui) = request_microphone_ui.upgrade() {
            update_microphone_authorization(&ui, microphone.request_authorization());
        }
    });

    let poll_ui = ui.as_weak();
    let authorization_poll = Timer::default();
    authorization_poll.start(
        TimerMode::Repeated,
        AUTHORIZATION_POLL_INTERVAL,
        move || {
            if let Some(ui) = poll_ui.upgrade() {
                update_authorizations(&ui, deliverer.authorization(), microphone.authorization());
            }
        },
    );

    let first_recording = Arc::new(AtomicBool::new(true));
    delivery_runtime::wire_result_actions(&result_overlay);
    wire_overlay_actions(
        &ui,
        &overlay,
        &result_overlay,
        Arc::clone(&recorder),
        Arc::clone(&recording_active),
        Arc::clone(&cancelled),
        Arc::clone(&asr),
    );
    let overlays = DictationOverlays::new(&overlay, &result_overlay);
    start_recording_shortcut(
        &ui,
        overlays,
        recorder,
        first_recording,
        recording_active,
        cancelled,
        asr,
    );
    ui.show()?;
    main_window::schedule_titlebar_integration(&ui);
    slint::run_event_loop()?;
    ui.hide()?;
    drop(authorization_poll);
    Ok(())
}

#[cfg(target_os = "macos")]
fn start_recording_shortcut(
    ui: &AppWindow,
    overlays: DictationOverlays,
    recorder: Arc<Mutex<MacOsAudioRecorder>>,
    first_recording: Arc<AtomicBool>,
    recording_active: Arc<AtomicBool>,
    cancelled: Arc<Mutex<CancelledRecordingStore>>,
    asr: Arc<AsrSessionController>,
) {
    let shortcut_ui = ui.as_weak();
    let shortcut_overlays = overlays;
    let monitor_active = Arc::clone(&recording_active);
    MacOsShortcutMonitor::start(monitor_active, move |action| match action {
        DictationShortcutAction::Toggle if recording_active.load(Ordering::Relaxed) => {
            finish_recording(
                shortcut_ui.clone(),
                shortcut_overlays.clone(),
                Arc::clone(&recorder),
                Arc::clone(&recording_active),
                Arc::clone(&asr),
            );
        }
        DictationShortcutAction::Toggle => begin_recording(
            &shortcut_ui,
            &shortcut_overlays,
            &recorder,
            &first_recording,
            &recording_active,
            &cancelled,
            &asr,
        ),
        DictationShortcutAction::Cancel => cancel_recording(
            &shortcut_ui,
            &shortcut_overlays.status,
            &recorder,
            &recording_active,
            &cancelled,
            &asr,
        ),
    });
}

#[cfg(target_os = "macos")]
fn begin_recording(
    ui: &slint::Weak<AppWindow>,
    overlays: &DictationOverlays,
    recorder: &Mutex<MacOsAudioRecorder>,
    first_recording: &Arc<AtomicBool>,
    recording_active: &Arc<AtomicBool>,
    cancelled: &Mutex<CancelledRecordingStore>,
    asr: &Arc<AsrSessionController>,
) {
    if let Some(result_overlay) = overlays.result.upgrade() {
        let _ = result_overlay.hide();
    }
    if let Ok(mut cancelled) = cancelled.lock() {
        cancelled.clear();
    }
    let metrics_ui = ui.clone();
    let metrics_overlay = overlays.status.clone();
    let on_metrics = Arc::new(move |metrics| {
        recording_metrics::update(&metrics_ui, &metrics_overlay, metrics);
    });
    let partial_ui = ui.clone();
    let on_partial = Arc::new(move |text: String| {
        let _ = partial_ui.upgrade_in_event_loop(move |ui| {
            ui.set_recording_detail(SharedString::from(text));
        });
    });
    match asr.start(on_partial) {
        Ok(()) => {}
        Err(error) => {
            let event_ui = ui.clone();
            let _ = event_ui.upgrade_in_event_loop(move |ui| {
                apply_asr_error(&ui, &error);
            });
            return;
        }
    }
    let streaming_asr = Arc::clone(asr);
    let on_audio_chunk = Arc::new(move |chunk: PcmChunk| {
        let _ = streaming_asr.push_audio(chunk.samples);
    });
    let result = recorder
        .lock()
        .map_err(|_| RecordingError::Capture("recorder lock was poisoned".to_owned()))
        .and_then(|mut recorder| recorder.start(on_metrics, on_audio_chunk));

    let event_ui = ui.clone();
    let event_overlay = overlays.status.clone();
    let first_recording = Arc::clone(first_recording);
    let recording_active = Arc::clone(recording_active);
    let show_device = first_recording.load(Ordering::Relaxed);
    let failed_asr = Arc::clone(asr);
    let _ = event_ui.upgrade_in_event_loop(move |ui| match result {
        Ok(started) => {
            play_feedback_sound(FeedbackSound::Start);
            recording_active.store(true, Ordering::Relaxed);
            first_recording.store(false, Ordering::Relaxed);
            apply_recording_started(&ui);
            if let Some(overlay) = event_overlay.upgrade() {
                first_recording_overlay(&overlay, &started.input_device_name, show_device);
            }
        }
        Err(error) => {
            failed_asr.cancel();
            play_feedback_sound(FeedbackSound::Failure);
            apply_recording_error(&ui, &error);
        }
    });
}

#[cfg(target_os = "macos")]
fn finish_recording(
    ui: slint::Weak<AppWindow>,
    overlays: DictationOverlays,
    recorder: Arc<Mutex<MacOsAudioRecorder>>,
    recording_active: Arc<AtomicBool>,
    asr: Arc<AsrSessionController>,
) {
    let processing_overlay = overlays.status.clone();
    let _ = ui.upgrade_in_event_loop(move |ui| {
        ui.set_recording_active(false);
        ui.set_recording_failed(false);
        ui.set_recording_complete(false);
        ui.set_recording_status(SharedString::from("处理中"));
        ui.set_recording_detail(SharedString::from("正在等待最终识别结果"));
        if let Some(overlay) = processing_overlay.upgrade() {
            overlay.set_mode(1);
            overlay.set_show_device(false);
        }
    });
    let failure_ui = ui.clone();
    let failure_recording_active = Arc::clone(&recording_active);
    if std::thread::Builder::new()
        .name("saymore-finish-dictation".to_owned())
        .spawn(move || {
            finish_recording_worker(ui, overlays, recorder, recording_active, asr);
        })
        .is_err()
    {
        failure_recording_active.store(false, Ordering::Relaxed);
        let _ = failure_ui.upgrade_in_event_loop(|ui| {
            apply_recording_error(
                &ui,
                &RecordingError::Capture("failed to start transcription worker".to_owned()),
            );
        });
    }
}

#[cfg(target_os = "macos")]
fn finish_recording_worker(
    ui: slint::Weak<AppWindow>,
    overlays: DictationOverlays,
    recorder: Arc<Mutex<MacOsAudioRecorder>>,
    recording_active: Arc<AtomicBool>,
    asr: Arc<AsrSessionController>,
) {
    let mut recorder = match recorder.lock() {
        Ok(recorder) if recorder.is_recording() => recorder,
        Ok(_) => return,
        Err(_) => {
            recording_active.store(false, Ordering::Relaxed);
            let _ = ui.upgrade_in_event_loop(|ui| {
                apply_recording_error(
                    &ui,
                    &RecordingError::Capture("recorder lock was poisoned".to_owned()),
                );
            });
            return;
        }
    };

    let recording_result = recorder.stop();
    let transcription_result = match recording_result {
        Ok(recording) => asr
            .finish()
            .map(|text| (recording, text))
            .map_err(FinishError::Recognition),
        Err(error) => {
            asr.cancel();
            Err(FinishError::Recording(error))
        }
    };
    recording_active.store(false, Ordering::Relaxed);
    let _ = ui.upgrade_in_event_loop(move |ui| match transcription_result {
        Ok((recording, transcript)) => {
            let transcript = normalize_transcript(&transcript);
            if transcript.is_empty() {
                play_feedback_sound(FeedbackSound::Failure);
                apply_asr_error(
                    &ui,
                    &SpeechRecognitionError::Protocol("empty transcript".to_owned()),
                );
                hide_overlay_after_delay(overlays.status);
                return;
            }
            delivery_runtime::schedule_delivery(
                ui.as_weak(),
                overlays.status,
                overlays.result,
                recording,
                transcript,
            );
        }
        Err(FinishError::Recording(RecordingError::NotRecording)) => {}
        Err(FinishError::Recording(error)) => {
            play_feedback_sound(FeedbackSound::Failure);
            apply_recording_error(&ui, &error);
            hide_overlay_after_delay(overlays.status);
        }
        Err(FinishError::Recognition(error)) => {
            play_feedback_sound(FeedbackSound::Failure);
            apply_asr_error(&ui, &error);
            hide_overlay_after_delay(overlays.status);
        }
    });
}

#[cfg(target_os = "macos")]
fn first_recording_overlay(overlay: &RecordingOverlay, device_name: &str, show_device: bool) {
    overlay.set_device_name(SharedString::from(device_name));
    overlay.set_show_device(show_device);
    overlay.set_mode(0);
    overlay.set_recording_level(0.0);
    if overlay.show().is_ok() {
        position_overlay(overlay);
    }

    if show_device {
        let overlay = overlay.as_weak();
        Timer::single_shot(Duration::from_secs(2), move || {
            if let Some(overlay) = overlay.upgrade() {
                overlay.set_show_device(false);
            }
        });
    }
}

#[cfg(target_os = "macos")]
fn position_overlay(overlay: &RecordingOverlay) {
    let handle = overlay.window().window_handle();
    let result = handle
        .window_handle()
        .map_err(|error| error.to_string())
        .and_then(|handle| match handle.as_raw() {
            RawWindowHandle::AppKit(handle) => unsafe {
                configure_overlay_window(handle.ns_view).map_err(|error| error.to_string())
            },
            _ => Err("the overlay does not have an AppKit window handle".to_owned()),
        });
    if let Err(error) = result {
        eprintln!("failed to position recording overlay: {error}");
    }
}

#[cfg(target_os = "macos")]
fn hide_overlay_after_delay(overlay: slint::Weak<RecordingOverlay>) {
    Timer::single_shot(Duration::from_millis(700), move || {
        if let Some(overlay) = overlay.upgrade() {
            let _ = overlay.hide();
        }
    });
}

#[cfg(target_os = "macos")]
fn wire_overlay_actions(
    ui: &AppWindow,
    overlay: &RecordingOverlay,
    result_overlay: &ResultOverlay,
    recorder: Arc<Mutex<MacOsAudioRecorder>>,
    recording_active: Arc<AtomicBool>,
    cancelled: Arc<Mutex<CancelledRecordingStore>>,
    asr: Arc<AsrSessionController>,
) {
    let finish_ui = ui.as_weak();
    let finish_overlays = DictationOverlays::new(overlay, result_overlay);
    let finish_recorder = Arc::clone(&recorder);
    let finish_active = Arc::clone(&recording_active);
    let finish_asr = Arc::clone(&asr);
    overlay.on_finish(move || {
        finish_recording(
            finish_ui.clone(),
            finish_overlays.clone(),
            Arc::clone(&finish_recorder),
            Arc::clone(&finish_active),
            Arc::clone(&finish_asr),
        );
    });

    let cancel_ui = ui.as_weak();
    let cancel_overlay = overlay.as_weak();
    let cancel_active = Arc::clone(&recording_active);
    let cancel_store = Arc::clone(&cancelled);
    let cancel_asr = Arc::clone(&asr);
    overlay.on_cancel(move || {
        cancel_recording(
            &cancel_ui,
            &cancel_overlay,
            &recorder,
            &cancel_active,
            &cancel_store,
            &cancel_asr,
        );
    });

    let undo_ui = ui.as_weak();
    let undo_overlay = overlay.as_weak();
    let undo_result_overlay = result_overlay.as_weak();
    let undo_asr = Arc::clone(&asr);
    let undo_active = Arc::clone(&recording_active);
    overlay.on_undo_cancel(move || {
        undo_cancelled_recording(
            &undo_ui,
            &undo_overlay,
            undo_result_overlay.clone(),
            &cancelled,
            Arc::clone(&undo_asr),
            Arc::clone(&undo_active),
        );
    });
}

#[cfg(target_os = "macos")]
fn cancel_recording(
    ui: &slint::Weak<AppWindow>,
    overlay: &slint::Weak<RecordingOverlay>,
    recorder: &Mutex<MacOsAudioRecorder>,
    recording_active: &AtomicBool,
    cancelled: &Arc<Mutex<CancelledRecordingStore>>,
    asr: &AsrSessionController,
) {
    recording_active.store(false, Ordering::Relaxed);
    asr.cancel();
    let result = recorder
        .lock()
        .map_err(|_| RecordingError::Capture("recorder lock was poisoned".to_owned()))
        .and_then(|mut recorder| recorder.stop());
    let cancel_overlay = overlay.clone();
    let cancelled = Arc::clone(cancelled);
    let _ = ui.upgrade_in_event_loop(move |ui| match result {
        Ok(recording) => {
            play_feedback_sound(FeedbackSound::Cancel);
            let generation = match cancelled.lock() {
                Ok(mut cancelled) => cancelled.retain(recording, Instant::now()),
                Err(_) => {
                    apply_recording_error(
                        &ui,
                        &RecordingError::Capture(
                            "cancelled recording lock was poisoned".to_owned(),
                        ),
                    );
                    return;
                }
            };
            ui.set_recording_active(false);
            ui.set_recording_complete(false);
            ui.set_recording_failed(false);
            ui.set_recording_level(0.0);
            ui.set_recording_status(SharedString::from("录音已取消"));
            ui.set_recording_detail(SharedString::from("2 秒内可以撤销"));
            if let Some(overlay) = cancel_overlay.upgrade() {
                overlay.set_show_device(false);
                overlay.set_mode(2);
            }
            schedule_cancel_expiration(cancel_overlay, cancelled, generation);
        }
        Err(RecordingError::NotRecording) => {}
        Err(error) => {
            play_feedback_sound(FeedbackSound::Failure);
            apply_recording_error(&ui, &error);
        }
    });
}

#[cfg(target_os = "macos")]
fn play_feedback_sound(sound: FeedbackSound) {
    let _ = MacOsFeedbackSoundPlayer.play(sound);
}

#[cfg(target_os = "macos")]
fn schedule_cancel_expiration(
    overlay: slint::Weak<RecordingOverlay>,
    cancelled: Arc<Mutex<CancelledRecordingStore>>,
    generation: u64,
) {
    Timer::single_shot(CANCEL_UNDO_WINDOW, move || {
        let expired = match cancelled.lock() {
            Ok(mut cancelled) => cancelled.expire(generation, Instant::now()),
            Err(_) => false,
        };
        if expired && let Some(overlay) = overlay.upgrade() {
            let _ = overlay.hide();
        }
    });
}

#[cfg(target_os = "macos")]
fn undo_cancelled_recording(
    ui: &slint::Weak<AppWindow>,
    overlay: &slint::Weak<RecordingOverlay>,
    result_overlay: slint::Weak<ResultOverlay>,
    cancelled: &Mutex<CancelledRecordingStore>,
    asr: Arc<AsrSessionController>,
    recording_active: Arc<AtomicBool>,
) {
    let recording = cancelled
        .lock()
        .ok()
        .and_then(|mut cancelled| cancelled.take(Instant::now()));
    let Some(recording) = recording else {
        return;
    };

    recording_active.store(true, Ordering::Relaxed);
    if let Some(overlay) = overlay.upgrade() {
        overlay.set_mode(1);
    }
    let event_ui = ui.clone();
    let event_overlay = overlay.clone();
    let _ = std::thread::Builder::new()
        .name("saymore-undo-dictation".to_owned())
        .spawn(move || {
            let partial_ui = event_ui.clone();
            let result = asr
                .start(Arc::new(move |text| {
                    let _ = partial_ui.upgrade_in_event_loop(move |ui| {
                        ui.set_recording_detail(SharedString::from(text));
                    });
                }))
                .and_then(|()| {
                    for chunk in recording.samples.chunks(1_600) {
                        asr.push_audio(chunk.to_vec())?;
                    }
                    asr.finish()
                });
            recording_active.store(false, Ordering::Relaxed);
            let _ = event_ui.upgrade_in_event_loop(move |ui| match result {
                Ok(transcript) => {
                    let transcript = normalize_transcript(&transcript);
                    delivery_runtime::schedule_delivery(
                        ui.as_weak(),
                        event_overlay,
                        result_overlay,
                        recording,
                        transcript,
                    );
                }
                Err(error) => {
                    apply_asr_error(&ui, &error);
                    hide_overlay_after_delay(event_overlay);
                }
            });
        });
}

#[cfg(not(target_os = "macos"))]
fn run() -> Result<(), Box<dyn Error>> {
    let ui = AppWindow::new()?;
    ui.set_authorization_status(SharedString::from("暂不支持当前平台"));
    ui.set_microphone_status(SharedString::from("暂不支持当前平台"));
    ui.run()?;
    Ok(())
}
