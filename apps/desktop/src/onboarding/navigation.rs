use super::*;

pub(super) fn wire_navigation(
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
        if advance_manual.load(Ordering::Acquire) {
            finish_step_change(&window, &advance_step, next, Ok(()));
        } else {
            let completion_window = advance_window.clone();
            let completion_step = Arc::clone(&advance_step);
            let result =
                advance_persistence.save(OnboardingStatus::InProgress, next, move |result| {
                    if let Some(window) = completion_window.upgrade() {
                        finish_step_change(&window, &completion_step, next, result);
                    }
                });
            if result.is_err() {
                window.set_action_status("save_failed".into());
            }
        }
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
        if back_manual.load(Ordering::Acquire) {
            finish_step_change(&window, &back_step, previous, Ok(()));
        } else {
            let completion_window = back_window.clone();
            let completion_step = Arc::clone(&back_step);
            let result =
                back_persistence.save(OnboardingStatus::InProgress, previous, move |result| {
                    if let Some(window) = completion_window.upgrade() {
                        finish_step_change(&window, &completion_step, previous, result);
                    }
                });
            if result.is_err() {
                window.set_action_status("save_failed".into());
            }
        }
    });
}

fn finish_step_change(
    window: &OnboardingWindow,
    step: &AtomicU8,
    next: OnboardingStep,
    result: Result<(), String>,
) {
    if result.is_err() {
        window.set_action_status("save_failed".into());
        return;
    }
    step.store(next.index(), Ordering::Release);
    window.set_step(i32::from(next.index()));
    window.set_action_status(SharedString::new());
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
        if skip_manual.load(Ordering::Acquire) {
            finish_onboarding(
                skip_window.clone(),
                skip_app.clone(),
                Arc::clone(&skip_active),
                Ok(()),
            );
        } else {
            let completion_window = skip_window.clone();
            let completion_app = skip_app.clone();
            let completion_active = Arc::clone(&skip_active);
            let result = skip_persistence.save(
                OnboardingStatus::Skipped,
                OnboardingStep::Welcome,
                move |result| {
                    finish_onboarding(completion_window, completion_app, completion_active, result);
                },
            );
            if result.is_err() {
                window.set_action_status("save_failed".into());
            }
        }
    });

    let finish_window = window.as_weak();
    let finish_app = app.as_weak();
    let finish_active = Arc::clone(&active);
    let finish_persistence = persistence;
    window.on_finish(move || {
        let Some(window) = finish_window.upgrade() else {
            return;
        };
        let completion_window = finish_window.clone();
        let completion_app = finish_app.clone();
        let completion_active = Arc::clone(&finish_active);
        let result = finish_persistence.save(
            OnboardingStatus::Completed,
            OnboardingStep::Complete,
            move |result| {
                finish_onboarding(completion_window, completion_app, completion_active, result);
            },
        );
        if result.is_err() {
            window.set_action_status("save_failed".into());
        }
    });
}

fn finish_onboarding(
    window: slint::Weak<OnboardingWindow>,
    app: slint::Weak<AppWindow>,
    active: Arc<AtomicBool>,
    result: Result<(), String>,
) {
    if result.is_err() {
        if let Some(window) = window.upgrade() {
            window.set_action_status("save_failed".into());
        }
        return;
    }
    active.store(false, Ordering::Release);
    if let Some(app) = app.upgrade() {
        let _ = show_main_window(&app);
    }
    schedule_hide(window);
}

impl Persistence {
    pub(super) fn save(
        &self,
        status: OnboardingStatus,
        step: OnboardingStep,
        completion: impl FnOnce(Result<(), String>) + Send + 'static,
    ) -> Result<(), LocalSettingsSubmissionError> {
        self.settings.submit(
            LocalSettingsChange::SetOnboardingProgress { status, step },
            move |result| completion(result.map(|_| ()).map_err(|error| error.to_string())),
        )
    }
}
