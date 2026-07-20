use super::*;
use slint::{ModelRc, VecModel};

pub(super) fn wire_shortcut_settings(
    ui: &AppWindow,
    settings: LocalSettingsHandle,
    controller: MacOsShortcutController,
) {
    let pending = Arc::new(AtomicBool::new(false));
    if let Ok(shortcuts) = controller.current() {
        apply_shortcut_ui(ui, &shortcuts, SharedString::new());
    }
    let capture_ui = ui.as_weak();
    let capture_settings = settings.clone();
    let capture_controller = controller.clone();
    let capture_pending = Arc::clone(&pending);
    ui.on_begin_shortcut_capture(move || {
        if capture_pending.load(Ordering::Acquire) {
            return;
        }
        begin_shortcut_capture(
            capture_ui.clone(),
            capture_settings.clone(),
            capture_controller.clone(),
            Arc::clone(&capture_pending),
            ShortcutCaptureTarget::Add,
        );
    });

    let edit_ui = ui.as_weak();
    let edit_settings = settings.clone();
    let edit_controller = controller.clone();
    let edit_pending = Arc::clone(&pending);
    ui.on_edit_shortcut(move |index| {
        if edit_pending.load(Ordering::Acquire) {
            return;
        }
        let Ok(index) = usize::try_from(index) else {
            return;
        };
        begin_shortcut_capture(
            edit_ui.clone(),
            edit_settings.clone(),
            edit_controller.clone(),
            Arc::clone(&edit_pending),
            ShortcutCaptureTarget::Replace(index),
        );
    });

    let remove_ui = ui.as_weak();
    ui.on_remove_shortcut(move |index| {
        if pending.load(Ordering::Acquire) {
            return;
        }
        remove_shortcut(
            remove_ui.clone(),
            settings.clone(),
            controller.clone(),
            Arc::clone(&pending),
            index,
        );
    });
}

#[derive(Clone, Copy)]
enum ShortcutCaptureTarget {
    Add,
    Replace(usize),
}

fn begin_shortcut_capture(
    ui: slint::Weak<AppWindow>,
    settings: LocalSettingsHandle,
    controller: MacOsShortcutController,
    pending: Arc<AtomicBool>,
    target: ShortcutCaptureTarget,
) {
    let receiver = match controller.begin_capture() {
        Ok(receiver) => receiver,
        Err(error) => {
            if let Some(window) = ui.upgrade() {
                window.set_shortcut_status(shortcut_error_label(&window, &error));
            }
            return;
        }
    };
    if let Some(window) = ui.upgrade() {
        window.set_shortcut_status(SharedString::new());
        window.set_shortcut_error_index(target.ui_error_index());
        window.set_shortcut_capturing(true);
    }
    let failure_ui = ui.clone();
    if thread::Builder::new()
        .name("saymore-capture-shortcut".to_owned())
        .spawn(move || {
            let Ok(result) = receiver.recv() else {
                return;
            };
            let _ = ui.upgrade_in_event_loop(move |window| match result {
                Ok(shortcut) => apply_captured_shortcut(
                    &window, settings, controller, pending, target, shortcut,
                ),
                Err(MacOsShortcutError::CaptureCancelled) => {
                    window.set_shortcut_capturing(false);
                    window.set_shortcut_status(SharedString::new());
                    window.set_shortcut_error_index(-2);
                }
                Err(error) => {
                    window.set_shortcut_capturing(false);
                    window.set_shortcut_status(shortcut_error_label(&window, &error));
                }
            });
        })
        .is_err()
        && let Some(window) = failure_ui.upgrade()
    {
        window.set_shortcut_capturing(false);
        window.set_shortcut_status(window.global::<Translations>().get_shortcut_save_failed());
    }
}

fn apply_captured_shortcut(
    window: &AppWindow,
    settings: LocalSettingsHandle,
    controller: MacOsShortcutController,
    pending: Arc<AtomicBool>,
    target: ShortcutCaptureTarget,
    shortcut: MacOsShortcut,
) {
    let Ok(mut shortcuts) = controller.current() else {
        window.set_shortcut_status(window.global::<Translations>().get_shortcut_save_failed());
        return;
    };
    window.set_shortcut_capturing(false);
    let duplicate = shortcuts
        .iter()
        .enumerate()
        .any(|(index, existing)| Some(index) != target.replaced_index() && existing == &shortcut);
    if duplicate {
        window.set_shortcut_status(window.global::<Translations>().get_shortcut_duplicate());
        return;
    }
    let status = if shortcut.likely_system_conflict() {
        window
            .global::<Translations>()
            .get_shortcut_possible_conflict()
    } else {
        SharedString::new()
    };
    match target {
        ShortcutCaptureTarget::Add => shortcuts.push(shortcut),
        ShortcutCaptureTarget::Replace(index) => {
            let Some(existing) = shortcuts.get_mut(index) else {
                window.set_shortcut_status(
                    window.global::<Translations>().get_shortcut_save_failed(),
                );
                return;
            };
            *existing = shortcut;
        }
    }
    persist_shortcuts(window, settings, controller, pending, shortcuts, status);
}

impl ShortcutCaptureTarget {
    fn replaced_index(self) -> Option<usize> {
        match self {
            Self::Add => None,
            Self::Replace(index) => Some(index),
        }
    }

    fn ui_error_index(self) -> i32 {
        match self {
            Self::Add => -1,
            Self::Replace(index) => i32::try_from(index).map_or(i32::MAX, |value| value),
        }
    }
}

fn remove_shortcut(
    ui: slint::Weak<AppWindow>,
    settings: LocalSettingsHandle,
    controller: MacOsShortcutController,
    pending: Arc<AtomicBool>,
    index: i32,
) {
    let Some(window) = ui.upgrade() else {
        return;
    };
    let Ok(mut shortcuts) = controller.current() else {
        window.set_shortcut_status(window.global::<Translations>().get_shortcut_save_failed());
        return;
    };
    let Ok(index) = usize::try_from(index) else {
        return;
    };
    if index >= shortcuts.len() {
        return;
    }
    shortcuts.remove(index);
    persist_shortcuts(
        &window,
        settings,
        controller,
        pending,
        shortcuts,
        SharedString::new(),
    );
}

fn persist_shortcuts(
    window: &AppWindow,
    settings: LocalSettingsHandle,
    controller: MacOsShortcutController,
    pending: Arc<AtomicBool>,
    shortcuts: Vec<MacOsShortcut>,
    status: SharedString,
) {
    if pending.swap(true, Ordering::AcqRel) {
        window.set_shortcut_status(window.global::<Translations>().get_shortcut_save_failed());
        return;
    }
    let ui = window.as_weak();
    let Ok(previous) = controller.current() else {
        pending.store(false, Ordering::Release);
        window.set_shortcut_status(window.global::<Translations>().get_shortcut_save_failed());
        return;
    };
    if controller.replace(shortcuts.clone()).is_err() {
        pending.store(false, Ordering::Release);
        window.set_shortcut_status(window.global::<Translations>().get_shortcut_save_failed());
        return;
    }
    apply_shortcut_ui(window, &shortcuts, status);
    let rollback_ui = ui.clone();
    let rollback_controller = controller.clone();
    let rollback_previous = previous.clone();
    let completion_pending = Arc::clone(&pending);
    let stored_shortcuts = shortcuts.iter().map(MacOsShortcut::storage_value).collect();
    let result = settings.submit(
        LocalSettingsChange::ReplaceDictationShortcuts(stored_shortcuts),
        move |result| {
            completion_pending.store(false, Ordering::Release);
            if let Err(error) = result {
                tracing::warn!(event = "shortcut.save_failed", reason = %error);
                rollback_shortcut(rollback_ui, rollback_controller, rollback_previous);
            }
        },
    );
    if let Err(error) = result {
        pending.store(false, Ordering::Release);
        tracing::warn!(event = "shortcut.submit_failed", reason = %error);
        rollback_shortcut(ui, controller, previous);
    }
}

fn rollback_shortcut(
    ui: slint::Weak<AppWindow>,
    controller: MacOsShortcutController,
    previous: Vec<MacOsShortcut>,
) {
    let _ = controller.replace(previous.clone());
    let _ = ui.upgrade_in_event_loop(move |window| {
        let status = window.global::<Translations>().get_shortcut_save_failed();
        apply_shortcut_ui(&window, &previous, status);
    });
}

fn apply_shortcut_ui(window: &AppWindow, shortcuts: &[MacOsShortcut], status: SharedString) {
    let labels: Vec<SharedString> = shortcuts
        .iter()
        .map(|shortcut| shortcut.display_label().into())
        .collect();
    window.set_shortcut_enabled(!labels.is_empty());
    window.set_shortcut_label(labels.first().cloned().unwrap_or_default());
    window.set_shortcut_labels(ModelRc::new(VecModel::from(labels)));
    window.set_shortcut_status(status);
    window.set_shortcut_error_index(-2);
    window.set_shortcut_capturing(false);
}

fn shortcut_error_label(window: &AppWindow, error: &MacOsShortcutError) -> SharedString {
    let translations = window.global::<Translations>();
    match error {
        MacOsShortcutError::Duplicate => translations.get_shortcut_duplicate(),
        MacOsShortcutError::MissingModifier => translations.get_shortcut_missing_modifier(),
        MacOsShortcutError::SystemReserved => translations.get_shortcut_reserved(),
        MacOsShortcutError::InvalidStorageValue => translations.get_shortcut_unsupported(),
        MacOsShortcutError::StateUnavailable => translations.get_shortcut_save_failed(),
        MacOsShortcutError::CaptureActive => translations.get_shortcut_capture_active(),
        MacOsShortcutError::CaptureCancelled => SharedString::new(),
    }
}
