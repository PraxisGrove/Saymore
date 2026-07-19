use super::*;
use crate::asr_runtime::AsrSessionController;
use crate::ui::{AppPage, Translations};
use template_app::SpeechRecognitionError;

const OVERLAY_REVEAL_DELAY: Duration = Duration::from_millis(17);
pub(crate) const OVERLAY_EXIT_DURATION: Duration = Duration::from_millis(110);

pub(super) fn start_recording_shortcut(
    ui: &AppWindow,
    overlays: DictationOverlays,
    controller: settings_actions::PlatformShortcutController,
    runtime: ShortcutRuntime,
) -> Result<PlatformShortcutMonitor, String> {
    let shortcut_ui = ui.as_weak();
    let shortcut_overlays = overlays;
    let monitor_session = Arc::clone(&runtime.session);
    let is_recording: Arc<dyn Fn() -> bool + Send + Sync> =
        Arc::new(move || monitor_session.is_recording());
    let shortcuts_enabled = shortcuts_enabled_callback(&runtime.paused);
    let on_permission_required = permission_required_callback(&runtime);
    start_platform_shortcut_monitor(
        is_recording,
        shortcuts_enabled,
        controller,
        move |action| match action {
            DictationShortcutAction::Toggle if (runtime.onboarding_toggle)() => {}
            DictationShortcutAction::Toggle => match runtime
                .session
                .request_toggle(runtime.paused.load(Ordering::Acquire))
            {
                DictationToggleAction::IgnorePaused => {
                    tracing::info!(
                        target: "saymore::diagnostics",
                        event = "recording.shortcut_ignored",
                        reason = "paused"
                    );
                }
                DictationToggleAction::IgnoreStarting(id)
                | DictationToggleAction::IgnoreFinishing(id) => {
                    tracing::info!(
                        target: "saymore::diagnostics",
                        event = "recording.shortcut_ignored",
                        dictation_id = ?id,
                        reason = "session_busy"
                    );
                }
                DictationToggleAction::Finish(id) => dictation_finish::finish_recording(
                    shortcut_ui.clone(),
                    shortcut_overlays.clone(),
                    Arc::clone(&runtime.recorder),
                    Arc::clone(&runtime.session),
                    id,
                    runtime.dictation.clone(),
                    Arc::clone(&runtime.feedback_sounds_enabled),
                ),
                DictationToggleAction::Start(id) => {
                    let shortcut_started = Instant::now();
                    let permission_started = Instant::now();
                    let allows_recording = runtime.microphone_access.allows_recording();
                    tracing::info!(
                        target: "saymore::diagnostics",
                        event = "recording.permission_checked",
                        dictation_id = %id,
                        duration_ms = permission_started.elapsed().as_millis()
                    );
                    if allows_recording {
                        begin_recording(
                            &shortcut_ui,
                            &shortcut_overlays,
                            &runtime,
                            id,
                            shortcut_started,
                        );
                    } else {
                        runtime.session.startup_failed();
                    }
                }
            },
            DictationShortcutAction::Cancel => recording_actions::cancel(
                &shortcut_ui,
                &shortcut_overlays.status,
                &shortcut_overlays.limit,
                &runtime.recorder,
                &runtime.session,
                &runtime.cancelled,
                &runtime.dictation.asr,
            ),
        },
        on_permission_required,
    )
}

fn shortcuts_enabled_callback(paused: &Arc<AtomicBool>) -> Arc<dyn Fn() -> bool + Send + Sync> {
    let paused = Arc::clone(paused);
    Arc::new(move || !paused.load(Ordering::Acquire))
}

type PermissionRequiredCallback = Box<dyn Fn() + Send>;

#[cfg(target_os = "macos")]
fn permission_required_callback(runtime: &ShortcutRuntime) -> PermissionRequiredCallback {
    let onboarding_toggle = Arc::clone(&runtime.onboarding_toggle);
    let prompt = runtime.accessibility_permission_prompt.clone();
    Box::new(move || {
        accessibility_permission_prompt::handle_required_shortcut(
            onboarding_toggle.as_ref(),
            || {
                prompt.show_required();
            },
        );
    })
}

#[cfg(target_os = "windows")]
fn permission_required_callback(_runtime: &ShortcutRuntime) -> PermissionRequiredCallback {
    Box::new(|| {})
}

fn begin_recording(
    ui: &slint::Weak<AppWindow>,
    overlays: &DictationOverlays,
    runtime: &ShortcutRuntime,
    id: DictationSessionId,
    startup_started: Instant,
) {
    if runtime.paused.load(Ordering::Acquire) {
        let _ = runtime.session.request_cancel();
        return;
    }
    if let Some(result_overlay) = overlays.result.upgrade() {
        let _ = result_overlay.hide();
    }
    if let Ok(mut cancelled) = runtime.cancelled.lock() {
        cancelled.clear();
    }
    let Some(asr_ms) = start_streaming_asr(ui, runtime, id, startup_started) else {
        return;
    };
    let on_metrics = create_recording_metrics_callback(ui, overlays, runtime, id);
    let streaming_asr = Arc::clone(&runtime.dictation.asr);
    let on_audio_chunk = Arc::new(move |chunk: PcmChunk| {
        let _ = streaming_asr.push_audio(chunk.samples);
    });
    let recorder_started = Instant::now();
    let result = runtime
        .recorder
        .lock()
        .map_err(|_| RecordingError::Capture("recorder lock was poisoned".to_owned()))
        .and_then(|mut recorder| recorder.start(on_metrics, on_audio_chunk));
    let recorder_ms = recorder_started.elapsed().as_millis();
    queue_recording_start_ui(
        ui,
        overlays,
        runtime,
        result,
        RecordingStartupTiming {
            id,
            asr_ms,
            recorder_ms,
            startup_started,
        },
    );
}

fn start_streaming_asr(
    ui: &slint::Weak<AppWindow>,
    runtime: &ShortcutRuntime,
    id: DictationSessionId,
    startup_started: Instant,
) -> Option<u128> {
    let partial_ui = ui.clone();
    let on_partial = Arc::new(move |text: String| {
        let _ = partial_ui.upgrade_in_event_loop(move |ui| {
            ui.set_recording_detail(SharedString::from(text));
        });
    });
    let asr_started = Instant::now();
    if let Err(error) = runtime.dictation.asr.start(id, on_partial) {
        runtime.session.startup_failed();
        tracing::warn!(
            target: "saymore::diagnostics",
            event = "recording.startup_failed",
            dictation_id = %id,
            stage = "asr",
            duration_ms = startup_started.elapsed().as_millis()
        );
        let reveal_configuration = should_reveal_asr_configuration(&error);
        let event_ui = ui.clone();
        let _ = event_ui.upgrade_in_event_loop(move |ui| {
            apply_asr_error(&ui, &error);
            if reveal_configuration {
                ui.set_current_page(AppPage::Models);
                status_tray::show_window(&ui.as_weak(), None);
            }
        });
        return None;
    }
    Some(asr_started.elapsed().as_millis())
}

fn queue_recording_start_ui(
    ui: &slint::Weak<AppWindow>,
    overlays: &DictationOverlays,
    runtime: &ShortcutRuntime,
    result: Result<RecordingStarted, RecordingError>,
    timing: RecordingStartupTiming,
) {
    let RecordingStartupTiming {
        id,
        asr_ms,
        recorder_ms,
        startup_started,
    } = timing;
    let event_ui = ui.clone();
    let context = RecordingStartUiContext {
        id,
        overlay: overlays.status.clone(),
        first_recording: Arc::clone(&runtime.first_recording),
        session: Arc::clone(&runtime.session),
        asr: Arc::clone(&runtime.dictation.asr),
        recorder: Arc::clone(&runtime.recorder),
        feedback_sounds_enabled: Arc::clone(&runtime.feedback_sounds_enabled),
        show_device: runtime.first_recording.load(Ordering::Relaxed),
        startup_started,
    };
    let queue_failure_session = Arc::clone(&runtime.session);
    let queue_failure_asr = Arc::clone(&runtime.dictation.asr);
    let queue_failure_recorder = Arc::clone(&runtime.recorder);
    let ui_queued = Instant::now();
    if event_ui
        .upgrade_in_event_loop(move |ui| {
            let outcome = if result.is_ok() { "ready" } else { "failed" };
            tracing::info!(
                target: "saymore::diagnostics",
                event = "recording.startup",
                dictation_id = %id,
                outcome,
                asr_ms,
                recorder_ms,
                ui_queue_ms = ui_queued.elapsed().as_millis(),
                total_ms = startup_started.elapsed().as_millis()
            );
            apply_recording_start_result(&ui, result, context);
        })
        .is_err()
    {
        let _ = queue_failure_session.request_cancel();
        queue_failure_asr.cancel();
        if let Ok(mut recorder) = queue_failure_recorder.lock() {
            let _ = recorder.stop();
        }
    }
}

struct RecordingStartupTiming {
    id: DictationSessionId,
    asr_ms: u128,
    recorder_ms: u128,
    startup_started: Instant,
}

struct RecordingStartUiContext {
    id: DictationSessionId,
    overlay: slint::Weak<RecordingOverlay>,
    first_recording: Arc<AtomicBool>,
    session: Arc<DictationSession>,
    asr: Arc<AsrSessionController>,
    recorder: RecorderHandle,
    feedback_sounds_enabled: Arc<AtomicBool>,
    show_device: bool,
    startup_started: Instant,
}

fn apply_recording_start_result(
    ui: &AppWindow,
    result: Result<RecordingStarted, RecordingError>,
    context: RecordingStartUiContext,
) {
    let started = match result {
        Ok(started) => started,
        Err(error) => {
            context.session.startup_failed();
            context.asr.cancel();
            apply_recording_error(ui, &error);
            return;
        }
    };
    if !context.session.recording_started() {
        context.asr.cancel();
        if let Ok(mut recorder) = context.recorder.lock() {
            let _ = recorder.stop();
        }
        return;
    }
    let feedback_started = Instant::now();
    if context.feedback_sounds_enabled.load(Ordering::Acquire) {
        play_feedback_sound(FeedbackSound::Start);
    }
    tracing::info!(
        target: "saymore::diagnostics",
        event = "recording.feedback_started",
        dictation_id = %context.id,
        duration_ms = feedback_started.elapsed().as_millis()
    );
    context.first_recording.store(false, Ordering::Relaxed);
    apply_recording_started(ui);
    if started.used_system_fallback {
        ui.set_microphone_selection_status(
            ui.global::<Translations>().get_microphone_system_fallback(),
        );
    }
    if let Some(overlay) = context.overlay.upgrade() {
        first_recording_overlay(
            &overlay,
            &started.input_device_name,
            context.show_device,
            context.id,
            context.startup_started,
        );
    }
}

fn create_recording_metrics_callback(
    ui: &slint::Weak<AppWindow>,
    overlays: &DictationOverlays,
    runtime: &ShortcutRuntime,
    id: DictationSessionId,
) -> Arc<dyn Fn(RecordingMetrics) + Send + Sync> {
    let metrics_ui = ui.clone();
    let metrics_overlay = overlays.status.clone();
    let limit_tracker = recording_limit::RecordingLimitTracker::default();
    let limit_overlay = overlays.limit.clone();
    let limit_finish_ui = ui.clone();
    let limit_finish_overlays = overlays.clone();
    let limit_recorder = Arc::clone(&runtime.recorder);
    let limit_session = Arc::clone(&runtime.session);
    let limit_dictation = runtime.dictation.clone();
    let limit_feedback_sounds = Arc::clone(&runtime.feedback_sounds_enabled);
    Arc::new(move |metrics| {
        recording_metrics::update(&metrics_ui, &metrics_overlay, metrics);
        match limit_tracker.observe(metrics.elapsed_ms) {
            recording_limit::RecordingLimitEvent::None => {}
            recording_limit::RecordingLimitEvent::Warn => {
                let warning = limit_overlay.clone();
                let _ = metrics_ui.upgrade_in_event_loop(move |_| {
                    if let Some(overlay) = warning.upgrade()
                        && let Err(error) = overlay_window::present(overlay.window())
                    {
                        tracing::warn!(
                            target: "saymore::diagnostics",
                            event = "recording.limit_warning_present_failed",
                            dictation_id = %id,
                            reason = %error
                        );
                    }
                });
            }
            recording_limit::RecordingLimitEvent::Finish => {
                if let Some(id) = limit_session.request_finish() {
                    dictation_finish::finish_recording(
                        limit_finish_ui.clone(),
                        limit_finish_overlays.clone(),
                        Arc::clone(&limit_recorder),
                        Arc::clone(&limit_session),
                        id,
                        limit_dictation.clone(),
                        Arc::clone(&limit_feedback_sounds),
                    );
                }
            }
        }
    })
}

fn first_recording_overlay(
    overlay: &RecordingOverlay,
    device_name: &str,
    show_device: bool,
    id: DictationSessionId,
    startup_started: Instant,
) {
    let _ = (device_name, show_device);
    let generation = overlay.get_session_generation().wrapping_add(1);
    overlay.set_session_generation(generation);
    overlay.set_mode(0);
    overlay.set_recording_level(0.0);
    overlay.set_revealed(false);
    let present_started = Instant::now();
    let result = overlay_window::present(overlay.window());
    tracing::info!(
        target: "saymore::diagnostics",
        event = "recording.overlay_presented",
        dictation_id = %id,
        present_ms = present_started.elapsed().as_millis(),
        total_ms = startup_started.elapsed().as_millis()
    );
    if let Err(error) = result {
        tracing::warn!(
            target: "saymore::diagnostics",
            event = "recording.overlay_present_failed",
            dictation_id = %id,
            reason = %error
        );
        return;
    }
    let overlay = overlay.as_weak();
    Timer::single_shot(OVERLAY_REVEAL_DELAY, move || {
        if let Some(overlay) = overlay.upgrade()
            && overlay_generation_matches(generation, overlay.get_session_generation())
        {
            overlay.set_revealed(true);
        }
    });
}

pub(crate) fn animate_overlay_hide(
    overlay: &RecordingOverlay,
    completion: impl FnOnce() + 'static,
) {
    let generation = overlay.get_session_generation();
    overlay.set_revealed(false);
    let overlay = overlay.as_weak();
    Timer::single_shot(OVERLAY_EXIT_DURATION, move || {
        if let Some(overlay) = overlay.upgrade()
            && overlay_generation_matches(generation, overlay.get_session_generation())
        {
            let _ = overlay.hide();
            overlay.set_mode(0);
            overlay.set_recording_level(0.0);
        }
        completion();
    });
}

pub(super) fn prepare_overlay_window(window: &slint::Window) {
    if let Err(error) = overlay_window::prepare(window) {
        tracing::warn!(event = "overlay.prepare_failed", reason = %error);
    }
}

pub(crate) fn hide_overlay_after_delay(overlay: slint::Weak<RecordingOverlay>) {
    let generation = overlay
        .upgrade()
        .map(|overlay| overlay.get_session_generation())
        .unwrap_or_default();
    Timer::single_shot(Duration::from_millis(700), move || {
        if let Some(overlay) = overlay.upgrade()
            && overlay_generation_matches(generation, overlay.get_session_generation())
        {
            animate_overlay_hide(&overlay, || {});
        }
    });
}

#[cfg(target_os = "macos")]
pub(super) struct PlatformShortcutMonitor;

#[cfg(target_os = "windows")]
pub(super) struct PlatformShortcutMonitor {
    _monitor: template_infra::WindowsShortcutMonitor,
}

fn should_reveal_asr_configuration(error: &SpeechRecognitionError) -> bool {
    matches!(error, SpeechRecognitionError::NotConfigured)
}

#[cfg(target_os = "macos")]
fn start_platform_shortcut_monitor(
    is_recording: Arc<dyn Fn() -> bool + Send + Sync>,
    shortcuts_enabled: Arc<dyn Fn() -> bool + Send + Sync>,
    controller: settings_actions::PlatformShortcutController,
    on_action: impl Fn(DictationShortcutAction) + Send + 'static,
    on_permission_required: impl Fn() + Send + 'static,
) -> Result<PlatformShortcutMonitor, String> {
    MacOsShortcutMonitor::start(
        is_recording,
        shortcuts_enabled,
        controller,
        on_action,
        on_permission_required,
    );
    Ok(PlatformShortcutMonitor)
}

#[cfg(target_os = "windows")]
fn start_platform_shortcut_monitor(
    is_recording: Arc<dyn Fn() -> bool + Send + Sync>,
    _shortcuts_enabled: Arc<dyn Fn() -> bool + Send + Sync>,
    controller: settings_actions::PlatformShortcutController,
    on_action: impl Fn(DictationShortcutAction) + Send + 'static,
    _on_permission_required: impl Fn() + Send + 'static,
) -> Result<PlatformShortcutMonitor, String> {
    template_infra::WindowsShortcutMonitor::start(is_recording, controller, on_action)
        .map(|monitor| PlatformShortcutMonitor { _monitor: monitor })
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod overlay_lifecycle_tests {
    use template_app::SpeechRecognitionError;

    use super::{overlay_generation_matches, should_reveal_asr_configuration};

    #[test]
    fn stale_overlay_work_cannot_affect_a_new_session() {
        assert!(overlay_generation_matches(7, 7));
        assert!(!overlay_generation_matches(7, 8));
    }

    #[test]
    fn missing_asr_configuration_requires_visible_setup() {
        assert!(should_reveal_asr_configuration(
            &SpeechRecognitionError::NotConfigured
        ));
        assert!(!should_reveal_asr_configuration(
            &SpeechRecognitionError::Authentication
        ));
    }
}
