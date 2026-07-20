use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use template_app::LocalSettingsChange;
use template_infra::{
    WindowsShortcut, WindowsShortcutController, WindowsShortcutError, WindowsShortcutUpdate,
};

use super::*;

pub(super) fn wire_shortcut_settings(
    ui: &AppWindow,
    settings: LocalSettingsHandle,
    controller: WindowsShortcutController,
) {
    if let Ok(shortcuts) = controller.current() {
        apply_shortcut_ui(ui, &shortcuts, SharedString::new());
    }
    let pending = Arc::new(AtomicBool::new(false));

    let add_ui = ui.as_weak();
    let add_settings = settings.clone();
    let add_controller = controller.clone();
    let add_pending = Arc::clone(&pending);
    ui.on_begin_shortcut_capture(move || {
        begin_shortcut_capture(
            add_ui.clone(),
            add_settings.clone(),
            add_controller.clone(),
            Arc::clone(&add_pending),
            ShortcutCaptureTarget::Add,
        );
    });

    let edit_ui = ui.as_weak();
    let edit_settings = settings.clone();
    let edit_controller = controller.clone();
    let edit_pending = Arc::clone(&pending);
    ui.on_edit_shortcut(move |index| {
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
    let remove_controller = controller;
    ui.on_remove_shortcut(move |index| {
        let Ok(index) = usize::try_from(index) else {
            return;
        };
        remove_shortcut(
            &remove_ui,
            settings.clone(),
            remove_controller.clone(),
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

fn begin_shortcut_capture(
    ui: slint::Weak<AppWindow>,
    settings: LocalSettingsHandle,
    controller: WindowsShortcutController,
    pending: Arc<AtomicBool>,
    target: ShortcutCaptureTarget,
) {
    let Some(window) = ui.upgrade() else {
        return;
    };
    if pending.load(Ordering::Acquire) {
        window.set_shortcut_status(
            window
                .global::<Translations>()
                .get_shortcut_capture_active(),
        );
        return;
    }
    window.set_shortcut_status(SharedString::new());
    window.set_shortcut_error_index(target.ui_error_index());
    window.set_shortcut_capturing(true);
    let capture_controller = controller.clone();
    if thread::Builder::new()
        .name("saymore-capture-windows-shortcut".to_owned())
        .spawn(move || {
            let captured = capture_controller.capture();
            let _ = ui.upgrade_in_event_loop(move |window| match captured {
                Ok(shortcut) => apply_captured_shortcut(
                    &window, settings, controller, pending, target, shortcut,
                ),
                Err(WindowsShortcutError::CaptureCancelled) => {
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
    {
        window.set_shortcut_capturing(false);
        window.set_shortcut_status(window.global::<Translations>().get_shortcut_save_failed());
    }
}

fn apply_captured_shortcut(
    window: &AppWindow,
    settings: LocalSettingsHandle,
    controller: WindowsShortcutController,
    pending: Arc<AtomicBool>,
    target: ShortcutCaptureTarget,
    shortcut: WindowsShortcut,
) {
    let Ok(mut shortcuts) = controller.current() else {
        window.set_shortcut_capturing(false);
        window.set_shortcut_status(window.global::<Translations>().get_shortcut_save_failed());
        return;
    };
    window.set_shortcut_capturing(false);
    if shortcuts
        .iter()
        .enumerate()
        .any(|(index, existing)| Some(index) != target.replaced_index() && existing == &shortcut)
    {
        window.set_shortcut_status(window.global::<Translations>().get_shortcut_duplicate());
        return;
    }
    let status = if shortcut.likely_system_conflict() {
        window
            .global::<Translations>()
            .get_windows_shortcut_possible_conflict()
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

fn remove_shortcut(
    ui: &slint::Weak<AppWindow>,
    settings: LocalSettingsHandle,
    controller: WindowsShortcutController,
    pending: Arc<AtomicBool>,
    index: usize,
) {
    let Some(window) = ui.upgrade() else {
        return;
    };
    let Ok(mut shortcuts) = controller.current() else {
        window.set_shortcut_status(window.global::<Translations>().get_shortcut_save_failed());
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
    controller: WindowsShortcutController,
    pending: Arc<AtomicBool>,
    shortcuts: Vec<WindowsShortcut>,
    status: SharedString,
) {
    if pending.swap(true, Ordering::AcqRel) {
        window.set_shortcut_status(
            window
                .global::<Translations>()
                .get_shortcut_capture_active(),
        );
        return;
    }
    let Ok(previous) = controller.current() else {
        pending.store(false, Ordering::Release);
        window.set_shortcut_status(window.global::<Translations>().get_shortcut_save_failed());
        return;
    };
    let update = match controller.stage_replace(shortcuts.clone()) {
        Ok(update) => update,
        Err(error) => {
            pending.store(false, Ordering::Release);
            window.set_shortcut_status(shortcut_error_label(window, &error));
            return;
        }
    };
    apply_shortcut_ui(window, &shortcuts, status);
    let stored_shortcuts = shortcuts
        .iter()
        .copied()
        .map(WindowsShortcut::storage_value)
        .collect();
    let rollback_ui = window.as_weak();
    let completion_controller = controller.clone();
    let completion_pending = Arc::clone(&pending);
    let completion_previous = previous.clone();
    let result = settings.submit(
        LocalSettingsChange::ReplaceDictationShortcuts(stored_shortcuts),
        move |result| {
            completion_pending.store(false, Ordering::Release);
            match result {
                Ok(_) => finish_update(
                    &rollback_ui,
                    &completion_controller,
                    update,
                    true,
                    &completion_previous,
                ),
                Err(error) => {
                    tracing::warn!(event = "shortcut.save_failed", reason = %error);
                    finish_update(
                        &rollback_ui,
                        &completion_controller,
                        update,
                        false,
                        &completion_previous,
                    );
                }
            }
        },
    );
    if let Err(error) = result {
        tracing::warn!(event = "shortcut.submit_failed", reason = %error);
        pending.store(false, Ordering::Release);
        finish_update(&window.as_weak(), &controller, update, false, &previous);
    }
}

fn finish_update(
    ui: &slint::Weak<AppWindow>,
    controller: &WindowsShortcutController,
    update: WindowsShortcutUpdate,
    commit: bool,
    previous: &[WindowsShortcut],
) {
    let result = if commit {
        controller.commit(update)
    } else {
        controller.rollback(update)
    };
    if let Some(window) = ui.upgrade() {
        if commit && result.is_ok() {
            return;
        }
        apply_failed_update_ui(&window, previous);
    }
    if let Err(error) = result {
        tracing::error!(event = "shortcut.runtime_finish_failed", reason = %error);
    }
}

fn apply_failed_update_ui(window: &AppWindow, previous: &[WindowsShortcut]) {
    let status = window.global::<Translations>().get_shortcut_save_failed();
    apply_shortcut_ui(window, previous, status);
}

fn apply_shortcut_ui(window: &AppWindow, shortcuts: &[WindowsShortcut], status: SharedString) {
    let labels: Vec<SharedString> = shortcuts
        .iter()
        .copied()
        .map(|shortcut| shortcut.display_label().into())
        .collect();
    window.set_shortcut_enabled(!labels.is_empty());
    window.set_shortcut_label(labels.first().cloned().unwrap_or_default());
    window.set_shortcut_labels(ModelRc::new(VecModel::from(labels)));
    window.set_shortcut_status(status);
    window.set_shortcut_error_index(-2);
    window.set_shortcut_capturing(false);
}

fn shortcut_error_label(window: &AppWindow, error: &WindowsShortcutError) -> SharedString {
    let translations = window.global::<Translations>();
    match error {
        WindowsShortcutError::Duplicate => translations.get_shortcut_duplicate(),
        WindowsShortcutError::MissingModifier => {
            translations.get_windows_shortcut_missing_modifier()
        }
        WindowsShortcutError::SystemReserved
        | WindowsShortcutError::RegistrationConflict { .. } => {
            translations.get_windows_shortcut_reserved()
        }
        WindowsShortcutError::CaptureActive | WindowsShortcutError::UpdateActive => {
            translations.get_shortcut_capture_active()
        }
        WindowsShortcutError::CaptureCancelled => SharedString::new(),
        WindowsShortcutError::InvalidStorageValue => {
            translations.get_windows_shortcut_unsupported()
        }
        WindowsShortcutError::StateUnavailable
        | WindowsShortcutError::RuntimeClosed
        | WindowsShortcutError::ThreadStart => translations.get_shortcut_save_failed(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_persistence_restores_previous_shortcut_in_the_ui() {
        let Ok(window) = AppWindow::new() else {
            panic!("test window should be created");
        };
        window.set_shortcut_label("Ctrl + Shift + F9".into());
        window.set_shortcut_capturing(true);

        apply_failed_update_ui(&window, &[WindowsShortcut::default()]);

        assert_eq!(SharedString::from("Right Alt"), window.get_shortcut_label());
        assert_eq!(
            window.global::<Translations>().get_shortcut_save_failed(),
            window.get_shortcut_status()
        );
        assert!(!window.get_shortcut_capturing());
    }
}
