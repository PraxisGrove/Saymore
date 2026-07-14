#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![cfg_attr(test, allow(clippy::panic))]

use std::{error::Error, process::ExitCode};
#[cfg(target_os = "macos")]
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use slint::{ComponentHandle, SharedString};
#[cfg(target_os = "macos")]
use slint::{Timer, TimerMode};
#[cfg(target_os = "macos")]
use template_app::{
    AudioRecorder, CancelledRecordingStore, MicrophonePermissionProvider, PcmChunk, RecordingError,
};
#[cfg(target_os = "macos")]
use template_app::{FeedbackSound, FeedbackSoundPlayer, TextDeliverer};
#[cfg(target_os = "macos")]
use template_infra::{
    AppEnvironment, AppInstanceGuard, AppPaths, DictationShortcutAction, JsonSettingsStore,
    MacOsAudioRecorder, MacOsFeedbackSoundPlayer, MacOsMicrophonePermission, MacOsShortcutMonitor,
    MacOsTextDeliverer, PlatformSecretStore, SqliteStorage,
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

use ui::AppWindow;
#[cfg(target_os = "macos")]
use ui::{MicrophoneIntroOverlay, MicrophonePermissionOverlay, RecordingOverlay, ResultOverlay};

#[cfg(target_os = "macos")]
mod app_environment;
#[cfg(target_os = "macos")]
mod asr_runtime;
#[cfg(target_os = "macos")]
mod delivery_runtime;
#[cfg(target_os = "macos")]
mod diagnostics;
#[cfg(target_os = "macos")]
mod dictation_finish;
#[cfg(target_os = "macos")]
mod home_stats;
#[cfg(target_os = "macos")]
mod local_data_ui;
#[cfg(target_os = "macos")]
mod main_window;
#[cfg(target_os = "macos")]
mod microphone_access;
#[cfg(target_os = "macos")]
mod overlay_window;
#[cfg(target_os = "macos")]
mod recording_metrics;
#[cfg(target_os = "macos")]
mod refinement_runtime;
#[cfg(target_os = "macos")]
mod settings_ui;
#[cfg(target_os = "macos")]
mod ui_status;

#[cfg(target_os = "macos")]
mod ui_tuning;

#[cfg(target_os = "macos")]
use asr_runtime::AsrSessionController;
#[cfg(target_os = "macos")]
use refinement_runtime::RefinementRuntime;
#[cfg(target_os = "macos")]
use ui_status::*;

#[cfg(target_os = "macos")]
const AUTHORIZATION_POLL_INTERVAL: Duration = Duration::from_secs(1);
#[cfg(target_os = "macos")]
const CANCEL_UNDO_WINDOW: Duration = Duration::from_secs(2);

#[cfg(target_os = "macos")]
#[derive(Clone)]
pub(crate) struct DictationOverlays {
    pub(crate) status: slint::Weak<RecordingOverlay>,
    pub(crate) result: slint::Weak<ResultOverlay>,
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

#[cfg(target_os = "macos")]
#[derive(Clone)]
pub(crate) struct TextProcessingServices {
    pub(crate) asr: Arc<AsrSessionController>,
    pub(crate) refinement: Arc<RefinementRuntime>,
    pub(crate) storage: Arc<SqliteStorage>,
    pub(crate) provider_config: Arc<JsonSettingsStore>,
}

#[cfg(target_os = "macos")]
struct ShortcutRuntime {
    recorder: Arc<Mutex<MacOsAudioRecorder>>,
    microphone_access: microphone_access::MicrophoneAccess,
    first_recording: Arc<AtomicBool>,
    recording_active: Arc<AtomicBool>,
    cancelled: Arc<Mutex<CancelledRecordingStore>>,
    processing: TextProcessingServices,
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
    let environment = app_environment::resolve()?;
    let paths = AppPaths::for_current_user(environment)?;
    if let Err(error) = diagnostics::init(paths.data_directory().join("logs")) {
        eprintln!("failed to initialize local diagnostics: {error}");
    }
    let _instance_guard = AppInstanceGuard::acquire(&paths.instance_lock())?;
    let settings_store = Arc::new(JsonSettingsStore::at_path(paths.provider_config()));
    settings_store.ensure_exists()?;
    let local_storage = Arc::new(SqliteStorage::start(
        paths.database(),
        Arc::new(PlatformSecretStore::new(environment)),
    )?);
    let ui = AppWindow::new()?;
    ui.set_development_environment(environment == AppEnvironment::Development);
    let overlay = RecordingOverlay::new()?;
    let result_overlay = ResultOverlay::new()?;
    let microphone_intro_overlay = MicrophoneIntroOverlay::new()?;
    let microphone_permission_overlay = MicrophonePermissionOverlay::new()?;
    let deliverer = MacOsTextDeliverer;
    let microphone = MacOsMicrophonePermission;
    let recorder = Arc::new(Mutex::new(MacOsAudioRecorder::default()));
    let recording_active = Arc::new(AtomicBool::new(false));
    let asr = Arc::new(AsrSessionController::new(Arc::clone(&settings_store)));
    let refinement = Arc::new(RefinementRuntime::new(Arc::clone(&settings_store))?);
    let processing = TextProcessingServices {
        asr,
        refinement,
        storage: Arc::clone(&local_storage),
        provider_config: Arc::clone(&settings_store),
    };
    let cancelled = Arc::new(Mutex::new(CancelledRecordingStore::new(CANCEL_UNDO_WINDOW)));
    update_authorizations(&ui, deliverer.authorization(), microphone.authorization());
    let microphone_access = microphone_access::wire(
        &ui,
        &microphone_intro_overlay,
        &microphone_permission_overlay,
        microphone,
    );
    settings_ui::wire(&ui, Arc::clone(&settings_store));
    home_stats::wire(&ui, Arc::clone(&local_storage));
    local_data_ui::wire(&ui, Arc::clone(&local_storage));
    ui_tuning::wire(&ui);

    let request_accessibility_ui = ui.as_weak();
    ui.on_request_authorization(move || {
        if let Some(ui) = request_accessibility_ui.upgrade() {
            update_accessibility_authorization(&ui, deliverer.request_authorization());
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
        processing.clone(),
    );
    let overlays = DictationOverlays::new(&overlay, &result_overlay);
    start_recording_shortcut(
        &ui,
        overlays,
        ShortcutRuntime {
            recorder,
            microphone_access,
            first_recording,
            recording_active,
            cancelled,
            processing,
        },
    );
    prepare_overlay_window(overlay.window());
    prepare_overlay_window(result_overlay.window());
    prepare_overlay_window(microphone_intro_overlay.window());
    prepare_overlay_window(microphone_permission_overlay.window());
    ui.show()?;
    main_window::schedule_titlebar_integration(&ui);
    slint::run_event_loop()?;
    ui.hide()?;
    drop(authorization_poll);
    Ok(())
}

#[cfg(target_os = "macos")]
fn start_recording_shortcut(ui: &AppWindow, overlays: DictationOverlays, runtime: ShortcutRuntime) {
    let shortcut_ui = ui.as_weak();
    let shortcut_overlays = overlays;
    let monitor_active = Arc::clone(&runtime.recording_active);
    MacOsShortcutMonitor::start(monitor_active, move |action| match action {
        DictationShortcutAction::Toggle if runtime.recording_active.load(Ordering::Relaxed) => {
            dictation_finish::finish_recording(
                shortcut_ui.clone(),
                shortcut_overlays.clone(),
                Arc::clone(&runtime.recorder),
                Arc::clone(&runtime.recording_active),
                runtime.processing.clone(),
            );
        }
        DictationShortcutAction::Toggle => {
            if runtime.microphone_access.allows_recording() {
                begin_recording(
                    &shortcut_ui,
                    &shortcut_overlays,
                    &runtime.recorder,
                    &runtime.first_recording,
                    &runtime.recording_active,
                    &runtime.cancelled,
                    &runtime.processing.asr,
                );
            }
        }
        DictationShortcutAction::Cancel => cancel_recording(
            &shortcut_ui,
            &shortcut_overlays.status,
            &runtime.recorder,
            &runtime.recording_active,
            &runtime.cancelled,
            &runtime.processing.asr,
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
            apply_recording_error(&ui, &error);
        }
    });
}

#[cfg(target_os = "macos")]
fn first_recording_overlay(overlay: &RecordingOverlay, device_name: &str, show_device: bool) {
    let _ = (device_name, show_device);
    overlay.set_mode(0);
    overlay.set_recording_level(0.0);
    if let Err(error) = overlay_window::present(overlay.window()) {
        tracing::warn!(event = "recording.overlay_present_failed", reason = %error);
    }
}

#[cfg(target_os = "macos")]
fn prepare_overlay_window(window: &slint::Window) {
    if let Err(error) = overlay_window::prepare(window) {
        tracing::warn!(event = "overlay.prepare_failed", reason = %error);
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn hide_overlay_after_delay(overlay: slint::Weak<RecordingOverlay>) {
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
    processing: TextProcessingServices,
) {
    let finish_ui = ui.as_weak();
    let finish_overlays = DictationOverlays::new(overlay, result_overlay);
    let finish_recorder = Arc::clone(&recorder);
    let finish_active = Arc::clone(&recording_active);
    let finish_processing = processing.clone();
    overlay.on_finish(move || {
        dictation_finish::finish_recording(
            finish_ui.clone(),
            finish_overlays.clone(),
            Arc::clone(&finish_recorder),
            Arc::clone(&finish_active),
            finish_processing.clone(),
        );
    });

    let cancel_ui = ui.as_weak();
    let cancel_overlay = overlay.as_weak();
    let cancel_active = Arc::clone(&recording_active);
    let cancel_store = Arc::clone(&cancelled);
    let cancel_asr = Arc::clone(&processing.asr);
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
    let undo_processing = processing;
    let undo_active = Arc::clone(&recording_active);
    overlay.on_undo_cancel(move || {
        undo_cancelled_recording(
            &undo_ui,
            &undo_overlay,
            undo_result_overlay.clone(),
            &cancelled,
            Arc::clone(&undo_active),
            undo_processing.clone(),
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
            ui.set_recording_attempted(false);
            ui.set_recording_level(0.0);
            ui.set_recording_status(SharedString::from("录音已取消"));
            ui.set_recording_detail(SharedString::from("2 秒内可以撤销"));
            if let Some(overlay) = cancel_overlay.upgrade() {
                overlay.set_mode(2);
            }
            schedule_cancel_expiration(cancel_overlay, cancelled, generation);
        }
        Err(RecordingError::NotRecording) => {}
        Err(error) => {
            apply_recording_error(&ui, &error);
        }
    });
}

#[cfg(target_os = "macos")]
pub(crate) fn play_feedback_sound(sound: FeedbackSound) {
    if let Err(error) = MacOsFeedbackSoundPlayer.play(sound) {
        eprintln!("failed to play feedback sound: {error}");
    }
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
    recording_active: Arc<AtomicBool>,
    processing: TextProcessingServices,
) {
    let recording = cancelled
        .lock()
        .ok()
        .and_then(|mut cancelled| cancelled.take(Instant::now()));
    let Some(recording) = recording else {
        return;
    };

    recording_active.store(true, Ordering::Relaxed);
    let plan = processing.refinement.plan();
    let processing_label = refinement_runtime::ProcessingActivity::Transcribing.label();
    if let Some(overlay) = overlay.upgrade() {
        overlay.set_mode(1);
        overlay.set_processing_label(SharedString::from(processing_label));
    }
    if let Some(ui) = ui.upgrade() {
        ui.set_recording_status(SharedString::from(processing_label));
        ui.set_recording_detail(SharedString::from(processing_label));
    }
    let event_ui = ui.clone();
    let event_overlay = overlay.clone();
    let failure_ui = ui.clone();
    let failure_overlay = overlay.clone();
    let failure_recording_active = Arc::clone(&recording_active);
    let spawn_result = std::thread::Builder::new()
        .name("saymore-undo-dictation".to_owned())
        .spawn(move || {
            let result = processing.asr.start(Arc::new(|_| {})).and_then(|()| {
                for chunk in recording.samples.chunks(1_600) {
                    processing.asr.push_audio(chunk.to_vec())?;
                }
                processing.asr.finish()
            });
            let result = result.and_then(|transcript| {
                let activity_ui = event_ui.clone();
                let activity_overlay = event_overlay.clone();
                let relevant_terms = refinement_runtime::relevant_terms_for_transcript(
                    &processing.storage,
                    &transcript,
                );
                processing
                    .refinement
                    .process_final_transcript(&transcript, plan, relevant_terms, move || {
                        dictation_finish::show_processing_activity(
                            &activity_ui,
                            &activity_overlay,
                            refinement_runtime::ProcessingActivity::Refining,
                        );
                    })
                    .map(|outcome| {
                        let history = dictation_finish::prepare_history(
                            &processing,
                            &recording,
                            &transcript,
                            outcome.llm_refined_text.as_deref(),
                            &outcome.processed,
                        );
                        (transcript, outcome.processed, history)
                    })
            });
            recording_active.store(false, Ordering::Relaxed);
            let _ = event_ui.upgrade_in_event_loop(move |ui| match result {
                Ok((_transcript, processed, history)) => {
                    if let Some(error) = history.error {
                        ui.set_history_status(SharedString::from(error));
                    }
                    delivery_runtime::schedule_delivery(delivery_runtime::DeliveryRequest {
                        ui: ui.as_weak(),
                        status_overlay: event_overlay,
                        result_overlay,
                        recording,
                        processed,
                        history: history.record,
                        storage: processing.storage,
                    });
                }
                Err(error) => {
                    apply_asr_error(&ui, &error);
                    hide_overlay_after_delay(event_overlay);
                }
            });
        });
    if spawn_result.is_err() {
        failure_recording_active.store(false, Ordering::Relaxed);
        let _ = failure_ui.upgrade_in_event_loop(move |ui| {
            apply_recording_error(
                &ui,
                &RecordingError::Capture("failed to start transcription worker".to_owned()),
            );
            hide_overlay_after_delay(failure_overlay);
        });
    }
}

#[cfg(not(target_os = "macos"))]
fn run() -> Result<(), Box<dyn Error>> {
    let ui = AppWindow::new()?;
    ui.set_authorization_status(SharedString::from("暂不支持当前平台"));
    ui.set_microphone_status(SharedString::from("暂不支持当前平台"));
    ui.run()?;
    Ok(())
}
