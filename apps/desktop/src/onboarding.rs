use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, Ordering},
};
use std::time::Duration;

#[cfg(any(target_os = "macos", target_os = "windows"))]
use slint::winit_030::{EventResult, WinitWindowAccessor, winit::event::WindowEvent};
use slint::{ComponentHandle, SharedString, Timer, TimerMode};
use template_app::{
    AccessibilityAuthorization, LocalSettings, LocalSettingsChange, MicrophoneAuthorization,
    MicrophonePermissionProvider, OnboardingStatus, OnboardingStep, PcmChunk, RecordingMetrics,
    TextDeliverer,
};
use template_infra::AppEnvironment;
#[cfg(target_os = "windows")]
use template_infra::{WindowsLaunchAtLogin, open_windows_microphone_privacy_settings};
#[cfg(target_os = "macos")]
use template_infra::{
    activate_application, launch_at_login_status, open_accessibility_privacy_settings,
    open_microphone_privacy_settings, set_launch_at_login,
};

use crate::{
    RecorderHandle,
    local_settings_runtime::{LocalSettingsHandle, LocalSettingsSubmissionError},
    main_window,
    permission_actions::{PermissionAction, microphone_permission_action},
    ui::{AppPage, AppWindow, OnboardingWindow},
};

const PERMISSION_POLL_INTERVAL: Duration = Duration::from_millis(500);
const WINDOW_HANDOFF_DELAY: Duration = Duration::from_millis(50);
const SOUND_DETECTED_LEVEL: f32 = 0.02;

mod navigation;

pub struct OnboardingRuntime {
    window: OnboardingWindow,
    initial_status: OnboardingStatus,
    initial_step: OnboardingStep,
    active: Arc<AtomicBool>,
    manual: Arc<AtomicBool>,
    shortcut: OnboardingShortcutHandler,
    persistence: Arc<Persistence>,
    _permission_poll: Timer,
}

#[derive(Clone)]
pub struct OnboardingShortcutHandler {
    window: slint::Weak<OnboardingWindow>,
    recorder: RecorderHandle,
    active: Arc<AtomicBool>,
    step: Arc<AtomicU8>,
    test_active: Arc<AtomicBool>,
    sound_detected: Arc<AtomicBool>,
}

struct Persistence {
    settings: LocalSettingsHandle,
}

impl OnboardingRuntime {
    pub fn new(
        app: &AppWindow,
        settings: &LocalSettings,
        environment: AppEnvironment,
        local_settings: LocalSettingsHandle,
        recorder: RecorderHandle,
        microphone: Arc<dyn MicrophonePermissionProvider>,
        deliverer: Arc<dyn TextDeliverer>,
    ) -> Result<Self, slint::PlatformError> {
        let window = OnboardingWindow::new()?;
        #[cfg(target_os = "windows")]
        crate::windows_window::integrate_onboarding(&window);
        let active = Arc::new(AtomicBool::new(false));
        let manual = Arc::new(AtomicBool::new(false));
        let initial_step = supported_step(settings.onboarding_step);
        let step = Arc::new(AtomicU8::new(initial_step.index()));
        let shortcut = OnboardingShortcutHandler {
            window: window.as_weak(),
            recorder,
            active: Arc::clone(&active),
            step: Arc::clone(&step),
            test_active: Arc::new(AtomicBool::new(false)),
            sound_detected: Arc::new(AtomicBool::new(false)),
        };
        let persistence = Arc::new(Persistence {
            settings: local_settings,
        });

        window.set_step(i32::from(initial_step.index()));
        window.set_default_shortcut_label(default_shortcut_label().into());
        window.set_show_accessibility_step(cfg!(target_os = "macos"));
        window.set_launch_at_login(launch_at_login_enabled(environment));
        update_permissions(&window, &*microphone, &*deliverer);
        navigation::wire_navigation(
            &window,
            app,
            Arc::clone(&persistence),
            Arc::clone(&active),
            Arc::clone(&manual),
            step,
            shortcut.clone(),
        );
        wire_permissions(&window, Arc::clone(&microphone), Arc::clone(&deliverer));
        wire_permission_focus_refresh(&window, Arc::clone(&microphone), Arc::clone(&deliverer));
        wire_launch_at_login(&window, environment);
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
                update_permissions(&window, &*microphone, &*deliverer);
            }
        });

        Ok(Self {
            window,
            initial_status: settings.onboarding_status,
            initial_step,
            active,
            manual,
            shortcut,
            persistence,
            _permission_poll: permission_poll,
        })
    }

    pub fn shortcut_handler(&self) -> OnboardingShortcutHandler {
        self.shortcut.clone()
    }

    pub fn present_initial(&self, app: &AppWindow) -> Result<(), slint::PlatformError> {
        if self.initial_status.should_present() {
            self.active.store(true, Ordering::Release);
            self.manual.store(false, Ordering::Release);
            self.window.set_step(i32::from(self.initial_step.index()));
            self.shortcut
                .step
                .store(self.initial_step.index(), Ordering::Release);
            if self.initial_status == OnboardingStatus::NotStarted {
                let window = self.window.as_weak();
                let result = self.persistence.save(
                    OnboardingStatus::InProgress,
                    self.initial_step,
                    move |result| {
                        if result.is_err()
                            && let Some(window) = window.upgrade()
                        {
                            window.set_action_status("save_failed".into());
                        }
                    },
                );
                if result.is_err() {
                    self.window.set_action_status("save_failed".into());
                }
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
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

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

fn wire_permissions(
    window: &OnboardingWindow,
    microphone: Arc<dyn MicrophonePermissionProvider>,
    deliverer: Arc<dyn TextDeliverer>,
) {
    let microphone_window = window.as_weak();
    let requested_microphone = Arc::clone(&microphone);
    let microphone_deliverer = Arc::clone(&deliverer);
    window.on_request_microphone(move || {
        match microphone_permission_action(requested_microphone.authorization()) {
            PermissionAction::Request => {
                requested_microphone.request_authorization();
            }
            PermissionAction::OpenSettings => {
                let _ = open_platform_microphone_privacy_settings();
            }
        }
        if let Some(window) = microphone_window.upgrade() {
            update_permissions(&window, &*requested_microphone, &*microphone_deliverer);
        }
    });

    let accessibility_window = window.as_weak();
    let accessibility_microphone = Arc::clone(&microphone);
    let accessibility_deliverer = Arc::clone(&deliverer);
    window.on_request_accessibility(move || {
        let _ = open_platform_accessibility_privacy_settings();
        if let Some(window) = accessibility_window.upgrade() {
            update_permissions(
                &window,
                &*accessibility_microphone,
                &*accessibility_deliverer,
            );
        }
    });

    let refresh_window = window.as_weak();
    window.on_refresh_permissions(move || {
        if let Some(window) = refresh_window.upgrade() {
            update_permissions(&window, &*microphone, &*deliverer);
        }
    });
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn wire_permission_focus_refresh(
    window: &OnboardingWindow,
    microphone: Arc<dyn MicrophonePermissionProvider>,
    deliverer: Arc<dyn TextDeliverer>,
) {
    let refresh_window = window.as_weak();
    window
        .window()
        .on_winit_window_event(move |_window, event| {
            if matches!(event, WindowEvent::Focused(true))
                && let Some(window) = refresh_window.upgrade()
            {
                update_permissions(&window, &*microphone, &*deliverer);
            }
            EventResult::Propagate
        });
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn wire_permission_focus_refresh(
    _window: &OnboardingWindow,
    _microphone: Arc<dyn MicrophonePermissionProvider>,
    _deliverer: Arc<dyn TextDeliverer>,
) {
}

#[cfg(target_os = "macos")]
fn wire_launch_at_login(window: &OnboardingWindow, _environment: AppEnvironment) {
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

#[cfg(target_os = "windows")]
fn wire_launch_at_login(window: &OnboardingWindow, environment: AppEnvironment) {
    let integration = WindowsLaunchAtLogin::for_current_executable(environment)
        .map(Arc::new)
        .ok();
    let launch_window = window.as_weak();
    window.on_set_launch_at_login(move |enabled| {
        let Some(window) = launch_window.upgrade() else {
            return;
        };
        let result = integration
            .as_ref()
            .ok_or(())
            .and_then(|integration| integration.set_enabled(enabled).map_err(|_| ()));
        if result.is_err() {
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
    microphone: &dyn MicrophonePermissionProvider,
    deliverer: &dyn TextDeliverer,
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

fn supported_step(step: OnboardingStep) -> OnboardingStep {
    #[cfg(target_os = "windows")]
    if step == OnboardingStep::Accessibility {
        return OnboardingStep::Complete;
    }
    step
}

fn show_main_window(app: &AppWindow) -> Result<(), slint::PlatformError> {
    app.set_current_page(AppPage::Home);
    app.show()?;
    app.window().request_redraw();
    main_window::schedule_titlebar_integration(app);
    if let Err(error) = activate_platform_window(app) {
        tracing::warn!(event = "onboarding.main_window_activate_failed", reason = %error);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn default_shortcut_label() -> &'static str {
    "Right Command"
}

#[cfg(target_os = "windows")]
fn default_shortcut_label() -> &'static str {
    "Right Alt"
}

#[cfg(target_os = "macos")]
fn launch_at_login_enabled(_environment: AppEnvironment) -> bool {
    launch_at_login_status().is_ok_and(|status| {
        matches!(
            status,
            template_infra::LaunchAtLoginStatus::Enabled
                | template_infra::LaunchAtLoginStatus::RequiresApproval
        )
    })
}

#[cfg(target_os = "windows")]
fn launch_at_login_enabled(environment: AppEnvironment) -> bool {
    WindowsLaunchAtLogin::for_current_executable(environment)
        .and_then(|integration| integration.is_enabled())
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn open_platform_microphone_privacy_settings() -> Result<(), String> {
    open_microphone_privacy_settings().map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
fn open_platform_accessibility_privacy_settings() -> Result<(), String> {
    open_accessibility_privacy_settings().map_err(|error| error.to_string())
}

#[cfg(not(target_os = "macos"))]
fn open_platform_accessibility_privacy_settings() -> Result<(), String> {
    Err("accessibility settings integration is unavailable on this platform".to_owned())
}

#[cfg(target_os = "windows")]
fn open_platform_microphone_privacy_settings() -> Result<(), String> {
    open_windows_microphone_privacy_settings().map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
fn activate_platform_window(_app: &AppWindow) -> Result<(), String> {
    activate_application().map_err(|error| error.to_string())
}

#[cfg(target_os = "windows")]
fn activate_platform_window(app: &AppWindow) -> Result<(), String> {
    crate::status_tray::activate_main_window(app)
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
    use super::{display_microphone_level, supported_step};
    use template_app::OnboardingStep;

    #[test]
    fn microphone_display_level_makes_quiet_speech_visible_and_stays_bounded() {
        assert_eq!(0.0, display_microphone_level(-1.0));
        assert!(display_microphone_level(0.02) > 0.3);
        assert_eq!(1.0, display_microphone_level(1.0));
        assert_eq!(1.0, display_microphone_level(2.0));
    }

    #[test]
    fn persisted_onboarding_step_is_supported_by_the_current_platform() {
        #[cfg(target_os = "windows")]
        assert_eq!(
            OnboardingStep::Complete,
            supported_step(OnboardingStep::Accessibility)
        );
        assert_eq!(
            OnboardingStep::Microphone,
            supported_step(OnboardingStep::Microphone)
        );
    }
}
