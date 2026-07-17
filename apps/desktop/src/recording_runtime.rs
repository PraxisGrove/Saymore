use super::*;

const OVERLAY_PRERENDER_DELAY: Duration = Duration::from_millis(17);

#[cfg(target_os = "macos")]
pub(super) fn start_recording_shortcut(
    ui: &AppWindow,
    overlays: DictationOverlays,
    controller: MacOsShortcutController,
    runtime: ShortcutRuntime,
) {
    let shortcut_ui = ui.as_weak();
    let shortcut_overlays = overlays;
    let monitor_session = Arc::clone(&runtime.session);
    let is_recording: Arc<dyn Fn() -> bool + Send + Sync> =
        Arc::new(move || monitor_session.is_recording());
    MacOsShortcutMonitor::start(is_recording, controller, move |action| match action {
        DictationShortcutAction::Toggle if runtime.onboarding.handle_toggle() => {}
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
            DictationToggleAction::IgnoreStarting | DictationToggleAction::IgnoreFinishing => {
                tracing::info!(
                    target: "saymore::diagnostics",
                    event = "recording.shortcut_ignored",
                    reason = "session_busy"
                );
            }
            DictationToggleAction::Finish(id) => dictation_finish::finish_recording(
                shortcut_ui.clone(),
                shortcut_overlays.clone(),
                Arc::clone(&runtime.recorder),
                Arc::clone(&runtime.session),
                id,
                runtime.processing.clone(),
            ),
            DictationToggleAction::Start => {
                let shortcut_started = Instant::now();
                let permission_started = Instant::now();
                let allows_recording = runtime.microphone_access.allows_recording();
                tracing::info!(
                    target: "saymore::diagnostics",
                    event = "recording.permission_checked",
                    duration_ms = permission_started.elapsed().as_millis()
                );
                if allows_recording {
                    begin_recording(&shortcut_ui, &shortcut_overlays, &runtime, shortcut_started);
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
            &runtime.processing.asr,
        ),
    });
}

#[cfg(target_os = "macos")]
fn begin_recording(
    ui: &slint::Weak<AppWindow>,
    overlays: &DictationOverlays,
    runtime: &ShortcutRuntime,
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
    let Some(asr_ms) = start_streaming_asr(ui, runtime, startup_started) else {
        return;
    };
    let on_metrics = create_recording_metrics_callback(ui, overlays, runtime);
    let streaming_asr = Arc::clone(&runtime.processing.asr);
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
        asr_ms,
        recorder_ms,
        startup_started,
    );
}

fn start_streaming_asr(
    ui: &slint::Weak<AppWindow>,
    runtime: &ShortcutRuntime,
    startup_started: Instant,
) -> Option<u128> {
    let partial_ui = ui.clone();
    let on_partial = Arc::new(move |text: String| {
        let _ = partial_ui.upgrade_in_event_loop(move |ui| {
            ui.set_recording_detail(SharedString::from(text));
        });
    });
    let asr_started = Instant::now();
    if let Err(error) = runtime.processing.asr.start(on_partial) {
        runtime.session.startup_failed();
        tracing::warn!(
            target: "saymore::diagnostics",
            event = "recording.startup_failed",
            stage = "asr",
            duration_ms = startup_started.elapsed().as_millis()
        );
        let event_ui = ui.clone();
        let _ = event_ui.upgrade_in_event_loop(move |ui| apply_asr_error(&ui, &error));
        return None;
    }
    Some(asr_started.elapsed().as_millis())
}

fn queue_recording_start_ui(
    ui: &slint::Weak<AppWindow>,
    overlays: &DictationOverlays,
    runtime: &ShortcutRuntime,
    result: Result<RecordingStarted, RecordingError>,
    asr_ms: u128,
    recorder_ms: u128,
    startup_started: Instant,
) {
    let event_ui = ui.clone();
    let context = RecordingStartUiContext {
        overlay: overlays.status.clone(),
        first_recording: Arc::clone(&runtime.first_recording),
        session: Arc::clone(&runtime.session),
        asr: Arc::clone(&runtime.processing.asr),
        recorder: Arc::clone(&runtime.recorder),
        feedback_sounds_enabled: Arc::clone(&runtime.processing.feedback_sounds_enabled),
        show_device: runtime.first_recording.load(Ordering::Relaxed),
        startup_started,
    };
    let queue_failure_session = Arc::clone(&runtime.session);
    let queue_failure_asr = Arc::clone(&runtime.processing.asr);
    let queue_failure_recorder = Arc::clone(&runtime.recorder);
    let ui_queued = Instant::now();
    if event_ui
        .upgrade_in_event_loop(move |ui| {
            let outcome = if result.is_ok() { "ready" } else { "failed" };
            tracing::info!(
                target: "saymore::diagnostics",
                event = "recording.startup",
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

struct RecordingStartUiContext {
    overlay: slint::Weak<RecordingOverlay>,
    first_recording: Arc<AtomicBool>,
    session: Arc<DictationSession>,
    asr: Arc<AsrSessionController>,
    recorder: Arc<Mutex<MacOsAudioRecorder>>,
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
            context.startup_started,
        );
    }
}

#[cfg(target_os = "macos")]
fn create_recording_metrics_callback(
    ui: &slint::Weak<AppWindow>,
    overlays: &DictationOverlays,
    runtime: &ShortcutRuntime,
) -> Arc<dyn Fn(RecordingMetrics) + Send + Sync> {
    let metrics_ui = ui.clone();
    let metrics_overlay = overlays.status.clone();
    let limit_tracker = recording_limit::RecordingLimitTracker::default();
    let limit_overlay = overlays.limit.clone();
    let limit_finish_ui = ui.clone();
    let limit_finish_overlays = overlays.clone();
    let limit_recorder = Arc::clone(&runtime.recorder);
    let limit_session = Arc::clone(&runtime.session);
    let limit_processing = runtime.processing.clone();
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
                        tracing::warn!(event = "recording.limit_warning_present_failed", reason = %error);
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
                        limit_processing.clone(),
                    );
                }
            }
        }
    })
}

#[cfg(target_os = "macos")]
fn first_recording_overlay(
    overlay: &RecordingOverlay,
    device_name: &str,
    show_device: bool,
    startup_started: Instant,
) {
    let _ = (device_name, show_device);
    let generation = overlay.get_session_generation().wrapping_add(1);
    overlay.set_session_generation(generation);
    overlay.set_mode(0);
    overlay.set_recording_level(0.0);
    overlay.window().request_redraw();
    let overlay = overlay.as_weak();
    Timer::single_shot(OVERLAY_PRERENDER_DELAY, move || {
        if let Some(overlay) = overlay.upgrade()
            && overlay_generation_matches(generation, overlay.get_session_generation())
        {
            let present_started = Instant::now();
            let result = overlay_window::present(overlay.window());
            tracing::info!(
                target: "saymore::diagnostics",
                event = "recording.overlay_presented",
                present_ms = present_started.elapsed().as_millis(),
                total_ms = startup_started.elapsed().as_millis()
            );
            if let Err(error) = result {
                tracing::warn!(event = "recording.overlay_present_failed", reason = %error);
            }
        }
    });
}

#[cfg(target_os = "macos")]
pub(crate) fn overlay_generation_matches(scheduled: i32, current: i32) -> bool {
    scheduled == current
}

#[cfg(target_os = "macos")]
pub(super) fn prepare_overlay_window(window: &slint::Window) {
    if let Err(error) = overlay_window::prepare(window) {
        tracing::warn!(event = "overlay.prepare_failed", reason = %error);
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn hide_overlay_after_delay(overlay: slint::Weak<RecordingOverlay>) {
    let generation = overlay
        .upgrade()
        .map(|overlay| overlay.get_session_generation())
        .unwrap_or_default();
    Timer::single_shot(Duration::from_millis(700), move || {
        if let Some(overlay) = overlay.upgrade()
            && overlay_generation_matches(generation, overlay.get_session_generation())
        {
            let _ = overlay.hide();
        }
    });
}

#[cfg(all(test, target_os = "macos"))]
mod overlay_lifecycle_tests {
    use super::overlay_generation_matches;

    #[test]
    fn stale_overlay_work_cannot_affect_a_new_session() {
        assert!(overlay_generation_matches(7, 7));
        assert!(!overlay_generation_matches(7, 8));
    }
}
