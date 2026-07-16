use super::*;

pub(super) fn wire_shortcut_settings(
    ui: &AppWindow,
    storage: Arc<SqliteStorage>,
    settings_guard: Arc<Mutex<()>>,
    controller: MacOsShortcutController,
) {
    if let Ok(shortcuts) = controller.current() {
        apply_shortcut_ui(ui, &shortcuts, SharedString::new());
    }
    let capture_ui = ui.as_weak();
    let capture_storage = Arc::clone(&storage);
    let capture_guard = Arc::clone(&settings_guard);
    let capture_controller = controller.clone();
    ui.on_begin_shortcut_capture(move || {
        begin_shortcut_capture(
            capture_ui.clone(),
            Arc::clone(&capture_storage),
            Arc::clone(&capture_guard),
            capture_controller.clone(),
            ShortcutCaptureTarget::Add,
        );
    });

    let edit_ui = ui.as_weak();
    let edit_storage = Arc::clone(&storage);
    let edit_guard = Arc::clone(&settings_guard);
    let edit_controller = controller.clone();
    ui.on_edit_shortcut(move |index| {
        let Ok(index) = usize::try_from(index) else {
            return;
        };
        begin_shortcut_capture(
            edit_ui.clone(),
            Arc::clone(&edit_storage),
            Arc::clone(&edit_guard),
            edit_controller.clone(),
            ShortcutCaptureTarget::Replace(index),
        );
    });

    let remove_ui = ui.as_weak();
    ui.on_remove_shortcut(move |index| {
        remove_shortcut(
            remove_ui.clone(),
            Arc::clone(&storage),
            Arc::clone(&settings_guard),
            controller.clone(),
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
    storage: Arc<SqliteStorage>,
    settings_guard: Arc<Mutex<()>>,
    controller: MacOsShortcutController,
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
                    &window,
                    storage,
                    settings_guard,
                    controller,
                    target,
                    shortcut,
                ),
                Err(MacOsShortcutError::CaptureCancelled) => {
                    window.set_shortcut_capturing(false);
                    window.set_shortcut_status(SharedString::new());
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
    storage: Arc<SqliteStorage>,
    settings_guard: Arc<Mutex<()>>,
    controller: MacOsShortcutController,
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
    persist_shortcuts(
        window,
        storage,
        settings_guard,
        controller,
        shortcuts,
        status,
    );
}

impl ShortcutCaptureTarget {
    fn replaced_index(self) -> Option<usize> {
        match self {
            Self::Add => None,
            Self::Replace(index) => Some(index),
        }
    }
}

fn remove_shortcut(
    ui: slint::Weak<AppWindow>,
    storage: Arc<SqliteStorage>,
    settings_guard: Arc<Mutex<()>>,
    controller: MacOsShortcutController,
    index: i32,
) {
    let Some(window) = ui.upgrade() else {
        return;
    };
    let Ok(mut shortcuts) = controller.current() else {
        window.set_shortcut_status(window.global::<Translations>().get_shortcut_save_failed());
        return;
    };
    if shortcuts.len() <= 1 {
        return;
    }
    let Ok(index) = usize::try_from(index) else {
        return;
    };
    if index >= shortcuts.len() {
        return;
    }
    shortcuts.remove(index);
    persist_shortcuts(
        &window,
        storage,
        settings_guard,
        controller,
        shortcuts,
        SharedString::new(),
    );
}

fn persist_shortcuts(
    window: &AppWindow,
    storage: Arc<SqliteStorage>,
    settings_guard: Arc<Mutex<()>>,
    controller: MacOsShortcutController,
    shortcuts: Vec<MacOsShortcut>,
    status: SharedString,
) {
    let ui = window.as_weak();
    let Ok(previous) = controller.current() else {
        window.set_shortcut_status(window.global::<Translations>().get_shortcut_save_failed());
        return;
    };
    if controller.replace(shortcuts.clone()).is_err() {
        window.set_shortcut_status(window.global::<Translations>().get_shortcut_save_failed());
        return;
    }
    apply_shortcut_ui(window, &shortcuts, status);
    let rollback_ui = ui.clone();
    let rollback_controller = controller.clone();
    let rollback_previous = previous.clone();
    let spawn = thread::Builder::new()
        .name("saymore-save-shortcut".to_owned())
        .spawn(move || {
            let result = settings_guard.lock().map_err(|_| ()).and_then(|_guard| {
                let mut settings = storage.load_settings().map_err(|_| ())?;
                settings.dictation_shortcuts =
                    shortcuts.iter().map(MacOsShortcut::storage_value).collect();
                storage.save_settings(settings).map_err(|_| ())
            });
            if result.is_err() {
                rollback_shortcut(rollback_ui, rollback_controller, rollback_previous);
            }
        });
    if spawn.is_err() {
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
        .map(|shortcut| shortcut_display_label(window, shortcut))
        .collect();
    if let Some(first) = labels.first() {
        window.set_shortcut_label(first.clone());
    }
    window.set_shortcut_labels(ModelRc::new(VecModel::from(labels)));
    window.set_shortcut_status(status);
    window.set_shortcut_capturing(false);
}

fn shortcut_display_label(window: &AppWindow, shortcut: &MacOsShortcut) -> SharedString {
    if shortcut.storage_value() == "right-command" {
        window.global::<Translations>().get_shortcut_right_command()
    } else {
        shortcut.display_label().into()
    }
}

fn shortcut_error_label(window: &AppWindow, error: &MacOsShortcutError) -> SharedString {
    let translations = window.global::<Translations>();
    match error {
        MacOsShortcutError::Duplicate => translations.get_shortcut_duplicate(),
        MacOsShortcutError::MissingModifier => translations.get_shortcut_missing_modifier(),
        MacOsShortcutError::InvalidStorageValue => translations.get_shortcut_unsupported(),
        MacOsShortcutError::StateUnavailable => translations.get_shortcut_save_failed(),
        MacOsShortcutError::CaptureActive => translations.get_shortcut_capture_active(),
        MacOsShortcutError::CaptureCancelled => SharedString::new(),
    }
}
