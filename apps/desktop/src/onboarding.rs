use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU8, Ordering},
};
use std::time::Duration;

use slint::{ComponentHandle, SharedString, Timer, TimerMode};
use template_app::{
    AccessibilityAuthorization, AudioRecorder, LocalSettings, LocalSettingsStore,
    MicrophoneAuthorization, MicrophonePermissionProvider, OnboardingStatus, OnboardingStep,
    PcmChunk, RecordingMetrics, TextDeliverer,
};
use template_infra::{
    MacOsAudioRecorder, MacOsMicrophonePermission, MacOsTextDeliverer, SqliteStorage,
    activate_application, launch_at_login_status, open_microphone_privacy_settings,
    set_launch_at_login,
};

use crate::{
    main_window,
    ui::{AppPage, AppWindow, OnboardingWindow},
};

const PERMISSION_POLL_INTERVAL: Duration = Duration::from_millis(500);
const WINDOW_HANDOFF_DELAY: Duration = Duration::from_millis(50);
const SOUND_DETECTED_LEVEL: f32 = 0.02;

pub struct OnboardingRuntime {
    window: OnboardingWindow,
    initial_status: OnboardingStatus,
    initial_step: OnboardingStep,
    active: Arc<AtomicBool>,
    manual: Arc<AtomicBool>,
    shortcut: OnboardingShortcutHandler,
    _permission_poll: Timer,
}

#[derive(Clone)]
pub struct OnboardingShortcutHandler {
    window: slint::Weak<OnboardingWindow>,
    recorder: Arc<Mutex<MacOsAudioRecorder>>,
    active: Arc<AtomicBool>,
    step: Arc<AtomicU8>,
    test_active: Arc<AtomicBool>,
    sound_detected: Arc<AtomicBool>,
}

struct Persistence {
    storage: Arc<SqliteStorage>,
    guard: Arc<Mutex<()>>,
}

impl OnboardingRuntime {
    pub fn new(
        app: &AppWindow,
        settings: &LocalSettings,
        storage: Arc<SqliteStorage>,
        settings_guard: Arc<Mutex<()>>,
        recorder: Arc<Mutex<MacOsAudioRecorder>>,
        microphone: MacOsMicrophonePermission,
        deliverer: MacOsTextDeliverer,
    ) -> Result<Self, slint::PlatformError> {
        let window = OnboardingWindow::new()?;
        let active = Arc::new(AtomicBool::new(false));
        let manual = Arc::new(AtomicBool::new(false));
        let step = Arc::new(AtomicU8::new(settings.onboarding_step.index()));
        let shortcut = OnboardingShortcutHandler {
            window: window.as_weak(),
            recorder,
            active: Arc::clone(&active),
            step: Arc::clone(&step),
            test_active: Arc::new(AtomicBool::new(false)),
            sound_detected: Arc::new(AtomicBool::new(false)),
        };
        let persistence = Arc::new(Persistence {
            storage,
            guard: settings_guard,
        });

        window.set_step(i32::from(settings.onboarding_step.index()));
        window.set_default_shortcut_label("Right Command".into());
        window.set_launch_at_login(launch_at_login_status().is_ok_and(|status| {
            matches!(
                status,
                template_infra::LaunchAtLoginStatus::Enabled
                    | template_infra::LaunchAtLoginStatus::RequiresApproval
            )
        }));
        update_permissions(&window, microphone, deliverer);
        wire_navigation(
            &window,
            app,
            Arc::clone(&persistence),
            Arc::clone(&active),
            Arc::clone(&manual),
            step,
            shortcut.clone(),
        );
        wire_permissions(&window, microphone, deliverer, shortcut.clone());
        wire_launch_at_login(&window);
        wire_manual_rerun(
            app,
            &window,
            Arc::clone(&active),
            Arc::clone(&manual),
            shortcut.clone(),
        );

        let poll_window = window.as_weak();
        let poll_active = Arc::clone(&active);
        let permission_poll = Timer::default();
        permission_poll.start(TimerMode::Repeated, PERMISSION_POLL_INTERVAL, move || {
            if poll_active.load(Ordering::Acquire)
                && let Some(window) = poll_window.upgrade()
            {
                update_permissions(&window, microphone, deliverer);
            }
        });

        Ok(Self {
            window,
            initial_status: settings.onboarding_status,
            initial_step: settings.onboarding_step,
            active,
            manual,
            shortcut,
            _permission_poll: permission_poll,
        })
    }

    pub fn shortcut_handler(&self) -> OnboardingShortcutHandler {
        self.shortcut.clone()
    }

    pub fn present_initial(
        &self,
        app: &AppWindow,
        storage: &SqliteStorage,
        settings_guard: &Mutex<()>,
    ) -> Result<(), slint::PlatformError> {
        if self.initial_status.should_present() {
            self.active.store(true, Ordering::Release);
            self.manual.store(false, Ordering::Release);
            self.window.set_step(i32::from(self.initial_step.index()));
            self.shortcut
                .step
                .store(self.initial_step.index(), Ordering::Release);
            if self.initial_status == OnboardingStatus::NotStarted
                && persist_state(
                    storage,
                    settings_guard,
                    OnboardingStatus::InProgress,
                    self.initial_step,
                )
                .is_err()
            {
                self.window.set_action_status("save_failed".into());
            }
            self.window.show()
        } else {
            show_main_window(app)
        }
    }

    pub fn hide(&self) {
        self.shortcut.stop_test();
        let _ = self.window.hide();
    }
}

impl OnboardingShortcutHandler {
    pub fn handle_toggle(&self) -> bool {
        if !self.active.load(Ordering::Acquire)
            || self.step.load(Ordering::Acquire) != OnboardingStep::Microphone.index()
        {
            return false;
        }
        self.toggle_test();
        true
    }

    fn toggle_test(&self) {
        if self
            .test_active
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            self.start_test();
        } else {
            self.stop_test();
        }
    }

    fn start_test(&self) {
        self.sound_detected.store(false, Ordering::Release);
        let metrics_window = self.window.clone();
        let sound_detected = Arc::clone(&self.sound_detected);
        let metrics = Arc::new(move |metrics: RecordingMetrics| {
            let level = metrics.level.clamp(0.0, 1.0);
            if level >= SOUND_DETECTED_LEVEL {
                sound_detected.store(true, Ordering::Release);
            }
            let display_level = display_microphone_level(level);
            let _ = metrics_window.upgrade_in_event_loop(move |window| {
                window.set_microphone_level(display_level);
            });
        });
        let chunks = Arc::new(|_chunk: PcmChunk| {});
        let result = self
            .recorder
            .lock()
            .map_err(|_| ())
            .and_then(|mut recorder| recorder.start(metrics, chunks).map_err(|_| ()));
        if result.is_err() {
            self.test_active.store(false, Ordering::Release);
            let _ = self.window.upgrade_in_event_loop(|window| {
                window.set_microphone_test_active(false);
                window.set_action_status("microphone_test_failed".into());
            });
            return;
        }
        let _ = self.window.upgrade_in_event_loop(|window| {
            window.set_action_status(SharedString::new());
            window.set_microphone_test_complete(false);
            window.set_microphone_test_active(true);
        });
    }

    fn stop_test(&self) {
        if !self.test_active.swap(false, Ordering::AcqRel) {
            return;
        }
        if let Ok(mut recorder) = self.recorder.lock() {
            let _ = recorder.cancel();
        }
        let complete = self.sound_detected.load(Ordering::Acquire);
        let _ = self.window.upgrade_in_event_loop(move |window| {
            window.set_microphone_level(0.0);
            window.set_microphone_test_active(false);
            window.set_microphone_test_complete(complete);
        });
    }
}

fn wire_navigation(
    window: &OnboardingWindow,
    app: &AppWindow,
    persistence: Arc<Persistence>,
    active: Arc<AtomicBool>,
    manual: Arc<AtomicBool>,
    step: Arc<AtomicU8>,
    shortcut: OnboardingShortcutHandler,
) {
    wire_step_navigation(
        window,
        Arc::clone(&persistence),
        Arc::clone(&manual),
        step,
        shortcut.clone(),
    );
    wire_completion_navigation(window, app, persistence, active, manual, shortcut);
}

fn wire_step_navigation(
    window: &OnboardingWindow,
    persistence: Arc<Persistence>,
    manual: Arc<AtomicBool>,
    step: Arc<AtomicU8>,
    shortcut: OnboardingShortcutHandler,
) {
    let advance_window = window.as_weak();
    let advance_persistence = Arc::clone(&persistence);
    let advance_manual = Arc::clone(&manual);
    let advance_step = Arc::clone(&step);
    let advance_shortcut = shortcut.clone();
    window.on_advance(move || {
        let Some(window) = advance_window.upgrade() else {
            return;
        };
        let next = u8::try_from(window.get_step())
            .ok()
            .and_then(|current| OnboardingStep::from_index(current.saturating_add(1)))
            .unwrap_or(OnboardingStep::Complete);
        advance_shortcut.stop_test();
        if !advance_manual.load(Ordering::Acquire)
            && advance_persistence
                .save(OnboardingStatus::InProgress, next)
                .is_err()
        {
            window.set_action_status("save_failed".into());
            return;
        }
        advance_step.store(next.index(), Ordering::Release);
        window.set_step(i32::from(next.index()));
        window.set_action_status(SharedString::new());
    });

    let back_window = window.as_weak();
    let back_persistence = Arc::clone(&persistence);
    let back_manual = Arc::clone(&manual);
    let back_step = Arc::clone(&step);
    let back_shortcut = shortcut;
    window.on_back(move || {
        let Some(window) = back_window.upgrade() else {
            return;
        };
        let current = u8::try_from(window.get_step()).unwrap_or_default();
        let previous = OnboardingStep::from_index(current.saturating_sub(1))
            .unwrap_or(OnboardingStep::Welcome);
        back_shortcut.stop_test();
        if !back_manual.load(Ordering::Acquire)
            && back_persistence
                .save(OnboardingStatus::InProgress, previous)
                .is_err()
        {
            window.set_action_status("save_failed".into());
            return;
        }
        back_step.store(previous.index(), Ordering::Release);
        window.set_step(i32::from(previous.index()));
        window.set_action_status(SharedString::new());
    });
}

fn wire_completion_navigation(
    window: &OnboardingWindow,
    app: &AppWindow,
    persistence: Arc<Persistence>,
    active: Arc<AtomicBool>,
    manual: Arc<AtomicBool>,
    shortcut: OnboardingShortcutHandler,
) {
    let skip_window = window.as_weak();
    let skip_app = app.as_weak();
    let skip_persistence = Arc::clone(&persistence);
    let skip_active = Arc::clone(&active);
    let skip_manual = Arc::clone(&manual);
    let skip_shortcut = shortcut;
    window.on_skip(move || {
        let Some(window) = skip_window.upgrade() else {
            return;
        };
        skip_shortcut.stop_test();
        if !skip_manual.load(Ordering::Acquire)
            && skip_persistence
                .save(OnboardingStatus::Skipped, OnboardingStep::Welcome)
                .is_err()
        {
            window.set_action_status("save_failed".into());
            return;
        }
        skip_active.store(false, Ordering::Release);
        if let Some(app) = skip_app.upgrade() {
            let _ = show_main_window(&app);
        }
        schedule_hide(skip_window.clone());
    });

    let finish_window = window.as_weak();
    let finish_app = app.as_weak();
    let finish_active = Arc::clone(&active);
    let finish_persistence = persistence;
    window.on_finish(move || {
        let Some(window) = finish_window.upgrade() else {
            return;
        };
        if finish_persistence
            .save(OnboardingStatus::Completed, OnboardingStep::Complete)
            .is_err()
        {
            window.set_action_status("save_failed".into());
            return;
        }
        finish_active.store(false, Ordering::Release);
        if let Some(app) = finish_app.upgrade() {
            let _ = show_main_window(&app);
        }
        schedule_hide(finish_window.clone());
    });
}

fn wire_permissions(
    window: &OnboardingWindow,
    microphone: MacOsMicrophonePermission,
    deliverer: MacOsTextDeliverer,
    shortcut: OnboardingShortcutHandler,
) {
    let microphone_window = window.as_weak();
    window.on_request_microphone(move || {
        match microphone.authorization() {
            MicrophoneAuthorization::NotDetermined => {
                microphone.request_authorization();
            }
            MicrophoneAuthorization::Denied | MicrophoneAuthorization::Restricted => {
                let _ = open_microphone_privacy_settings();
            }
            MicrophoneAuthorization::Granted => {}
        }
        if let Some(window) = microphone_window.upgrade() {
            update_permissions(&window, microphone, deliverer);
        }
    });

    let test_shortcut = shortcut;
    window.on_toggle_microphone_test(move || test_shortcut.toggle_test());

    let accessibility_window = window.as_weak();
    window.on_request_accessibility(move || {
        deliverer.request_authorization();
        if let Some(window) = accessibility_window.upgrade() {
            update_permissions(&window, microphone, deliverer);
        }
    });
}

fn wire_launch_at_login(window: &OnboardingWindow) {
    let launch_window = window.as_weak();
    window.on_set_launch_at_login(move |enabled| {
        let Some(window) = launch_window.upgrade() else {
            return;
        };
        if set_launch_at_login(enabled).is_err() {
            window.set_launch_at_login(!enabled);
            window.set_action_status("save_failed".into());
        } else {
            window.set_action_status(SharedString::new());
        }
    });
}

fn wire_manual_rerun(
    app: &AppWindow,
    window: &OnboardingWindow,
    active: Arc<AtomicBool>,
    manual: Arc<AtomicBool>,
    shortcut: OnboardingShortcutHandler,
) {
    let app_window = app.as_weak();
    let onboarding = window.as_weak();
    app.on_rerun_onboarding(move || {
        let Some(onboarding) = onboarding.upgrade() else {
            return;
        };
        shortcut.stop_test();
        manual.store(true, Ordering::Release);
        active.store(true, Ordering::Release);
        shortcut.step.store(0, Ordering::Release);
        onboarding.set_step(0);
        onboarding.set_microphone_test_active(false);
        onboarding.set_microphone_test_complete(false);
        onboarding.set_microphone_level(0.0);
        onboarding.set_action_status(SharedString::new());
        let _ = onboarding.show();
        onboarding.window().request_redraw();
        schedule_hide(app_window.clone());
    });
}

fn update_permissions(
    window: &OnboardingWindow,
    microphone: MacOsMicrophonePermission,
    deliverer: MacOsTextDeliverer,
) {
    let microphone_authorization = microphone.authorization();
    window.set_microphone_authorized(microphone_authorization == MicrophoneAuthorization::Granted);
    window.set_microphone_status(microphone_status(microphone_authorization).into());
    let accessibility = deliverer.authorization();
    window.set_accessibility_authorized(accessibility == AccessibilityAuthorization::Granted);
    window.set_accessibility_status(accessibility_status(accessibility).into());
}

fn microphone_status(status: MicrophoneAuthorization) -> &'static str {
    match status {
        MicrophoneAuthorization::NotDetermined => "not_determined",
        MicrophoneAuthorization::Granted => "granted",
        MicrophoneAuthorization::Denied => "denied",
        MicrophoneAuthorization::Restricted => "restricted",
    }
}

fn accessibility_status(status: AccessibilityAuthorization) -> &'static str {
    match status {
        AccessibilityAuthorization::Granted => "granted",
        AccessibilityAuthorization::Denied => "denied",
    }
}

fn persist_state(
    storage: &SqliteStorage,
    guard: &Mutex<()>,
    status: OnboardingStatus,
    step: OnboardingStep,
) -> Result<(), ()> {
    let _guard = guard.lock().map_err(|_| ())?;
    let mut settings = storage.load_settings().map_err(|_| ())?;
    settings.onboarding_status = status;
    settings.onboarding_step = step;
    storage.save_settings(settings).map_err(|_| ())
}

impl Persistence {
    fn save(&self, status: OnboardingStatus, step: OnboardingStep) -> Result<(), ()> {
        persist_state(&self.storage, &self.guard, status, step)
    }
}

fn show_main_window(app: &AppWindow) -> Result<(), slint::PlatformError> {
    app.set_current_page(AppPage::Home);
    app.show()?;
    app.window().request_redraw();
    main_window::schedule_titlebar_integration(app);
    if let Err(error) = activate_application() {
        tracing::warn!(event = "onboarding.main_window_activate_failed", reason = %error);
    }
    Ok(())
}

fn display_microphone_level(level: f32) -> f32 {
    (level.clamp(0.0, 1.0) * 6.0).sqrt().clamp(0.0, 1.0)
}

fn schedule_hide<T: ComponentHandle + 'static>(component: slint::Weak<T>) {
    Timer::single_shot(WINDOW_HANDOFF_DELAY, move || {
        if let Some(component) = component.upgrade() {
            let _ = component.hide();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::display_microphone_level;

    #[test]
    fn microphone_display_level_makes_quiet_speech_visible_and_stays_bounded() {
        assert_eq!(0.0, display_microphone_level(-1.0));
        assert!(display_microphone_level(0.02) > 0.3);
        assert_eq!(1.0, display_microphone_level(1.0));
        assert_eq!(1.0, display_microphone_level(2.0));
    }
}
