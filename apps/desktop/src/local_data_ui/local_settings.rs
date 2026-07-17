use super::*;

pub(super) fn wire_local_settings(
    ui: &AppWindow,
    storage: Arc<SqliteStorage>,
    state: Arc<Mutex<UiDataState>>,
    settings: LocalSettingsHandle,
    recorder: Arc<Mutex<MacOsAudioRecorder>>,
) {
    wire_history_settings(ui, Arc::clone(&storage), state, settings.clone());
    wire_microphone_settings(ui, storage, settings, recorder);
}

fn wire_history_settings(
    ui: &AppWindow,
    storage: Arc<SqliteStorage>,
    state: Arc<Mutex<UiDataState>>,
    settings: LocalSettingsHandle,
) {
    let history_ui = ui.as_weak();
    let history_settings = settings.clone();
    ui.on_set_history_enabled(move |enabled| {
        let completion_ui = history_ui.clone();
        let failure_ui = history_ui.clone();
        let result = history_settings.submit(
            LocalSettingsChange::SetHistoryEnabled(enabled),
            move |result| match result {
                Ok(committed) => {
                    if let Some(ui) = completion_ui.upgrade() {
                        ui.set_history_enabled(committed.history_enabled);
                        ui.invoke_refresh_history();
                    }
                }
                Err(error) => apply_settings_save_error(&completion_ui, "history.enabled", error),
            },
        );
        if let Err(error) = result {
            apply_settings_submit_error(&failure_ui, "history.enabled", error);
        }
    });
    let retention_ui = ui.as_weak();
    let retention_store = Arc::clone(&storage);
    let retention_state = Arc::clone(&state);
    let retention_settings = settings;
    ui.on_set_history_retention(move |selection| {
        let (enabled, retention) = match selection {
            UiHistoryRetention::Never => (false, HistoryRetention::SevenDays),
            UiHistoryRetention::OneDay => (true, HistoryRetention::OneDay),
            UiHistoryRetention::SevenDays => (true, HistoryRetention::SevenDays),
            UiHistoryRetention::ThirtyDays => (true, HistoryRetention::ThirtyDays),
            UiHistoryRetention::Forever => (true, HistoryRetention::Forever),
        };
        let completion_ui = retention_ui.clone();
        let failure_ui = retention_ui.clone();
        let cleanup_store = Arc::clone(&retention_store);
        let cleanup_state = Arc::clone(&retention_state);
        let result = retention_settings.submit(
            LocalSettingsChange::SetHistoryPolicy { enabled, retention },
            move |result| match result {
                Ok(committed) => {
                    if let Some(ui) = completion_ui.upgrade() {
                        ui.set_history_enabled(committed.history_enabled);
                        ui.set_history_retention(selection);
                    }
                    refresh_history_after_cleanup(completion_ui, cleanup_store, cleanup_state);
                }
                Err(error) => apply_settings_save_error(&completion_ui, "history.retention", error),
            },
        );
        if let Err(error) = result {
            apply_settings_submit_error(&failure_ui, "history.retention", error);
        }
    });
}

fn wire_microphone_settings(
    ui: &AppWindow,
    storage: Arc<SqliteStorage>,
    settings: LocalSettingsHandle,
    recorder: Arc<Mutex<MacOsAudioRecorder>>,
) {
    let microphone_ui = ui.as_weak();
    let microphone_store = Arc::clone(&storage);
    let microphone_settings = settings;
    let microphone_recorder = recorder;
    ui.on_select_microphone(move |id, name| {
        save_microphone_selection_async(
            microphone_ui.clone(),
            Arc::clone(&microphone_store),
            microphone_settings.clone(),
            Arc::clone(&microphone_recorder),
            (!id.is_empty()).then(|| id.to_string()),
            (!name.is_empty()).then(|| name.to_string()),
        );
    });

    let refresh_microphone_ui = ui.as_weak();
    let refresh_microphone_store = Arc::clone(&storage);
    ui.on_refresh_microphones(move || {
        refresh_microphone_devices_async(
            refresh_microphone_ui.clone(),
            Arc::clone(&refresh_microphone_store),
        );
    });
}

fn save_microphone_selection_async(
    ui: slint::Weak<AppWindow>,
    storage: Arc<SqliteStorage>,
    settings: LocalSettingsHandle,
    recorder: Arc<Mutex<MacOsAudioRecorder>>,
    preferred_id: Option<String>,
    preferred_name: Option<String>,
) {
    let selection = match (preferred_id, preferred_name) {
        (Some(id), Some(name)) => MicrophoneSelection::Specific { id, name },
        (None, None) => MicrophoneSelection::Automatic,
        _ => {
            apply_microphone_settings_error(&ui, "incomplete microphone selection");
            return;
        }
    };
    let saved_id = match &selection {
        MicrophoneSelection::Automatic => None,
        MicrophoneSelection::Specific { id, .. } => Some(id.clone()),
    };
    let failure_ui = ui.clone();
    let result = settings.submit(
        LocalSettingsChange::SelectMicrophone(selection),
        move |result| match result {
            Ok(committed) => {
                if let Ok(mut recorder) = recorder.lock() {
                    recorder.set_preferred_input_device_id(saved_id);
                } else {
                    tracing::error!(
                        event = "microphone.selection_update_failed",
                        reason = "recorder lock was poisoned"
                    );
                }
                let refresh_ui = ui.clone();
                if let Some(ui) = ui.upgrade() {
                    apply_microphone_devices(&ui, &committed, Vec::new());
                    ui.set_microphone_selection_status(SharedString::new());
                }
                refresh_microphone_devices_async(refresh_ui, storage);
            }
            Err(error) => apply_microphone_settings_error(&ui, error),
        },
    );
    if let Err(error) = result {
        apply_microphone_settings_error(&failure_ui, error);
    }
}

pub(super) fn refresh_microphone_devices_async(
    ui: slint::Weak<AppWindow>,
    storage: Arc<SqliteStorage>,
) {
    if let Some(window) = ui.upgrade() {
        window.set_microphone_devices_loading(true);
    }
    spawn_named("saymore-list-microphones", move || {
        let result = storage
            .load_settings()
            .map_err(|error| error.to_string())
            .and_then(|settings| {
                MacOsAudioRecorder::input_devices()
                    .map(|devices| (settings, devices))
                    .map_err(|error| error.to_string())
            });
        let _ = ui.upgrade_in_event_loop(move |ui| {
            ui.set_microphone_devices_loading(false);
            match result {
                Ok((settings, devices)) => apply_microphone_devices(&ui, &settings, devices),
                Err(error) => {
                    tracing::warn!(event = "microphone.list_failed", reason = %error);
                    ui.set_microphone_selection_status(
                        ui.global::<Translations>().get_microphone_load_failed(),
                    );
                }
            }
        });
    });
}

fn apply_settings_save_error(
    ui: &slint::Weak<AppWindow>,
    operation: &'static str,
    error: impl std::fmt::Display,
) {
    tracing::warn!(event = "settings.save_failed", operation, reason = %error);
    if let Some(ui) = ui.upgrade() {
        ui.set_history_status(ui.global::<Translations>().get_settings_save_failed());
    }
}

fn apply_settings_submit_error(
    ui: &slint::Weak<AppWindow>,
    operation: &'static str,
    error: impl std::fmt::Display,
) {
    tracing::warn!(event = "settings.submit_failed", operation, reason = %error);
    if let Some(ui) = ui.upgrade() {
        ui.set_history_status(ui.global::<Translations>().get_settings_save_failed());
    }
}

fn apply_microphone_settings_error(ui: &slint::Weak<AppWindow>, error: impl std::fmt::Display) {
    tracing::warn!(event = "microphone.selection_save_failed", reason = %error);
    if let Some(ui) = ui.upgrade() {
        ui.set_microphone_selection_status(ui.global::<Translations>().get_settings_save_failed());
    }
}

pub(super) fn schedule_history_cleanup(
    ui: slint::Weak<AppWindow>,
    storage: Arc<SqliteStorage>,
    state: Arc<Mutex<UiDataState>>,
) {
    Timer::single_shot(Duration::from_secs(24 * 60 * 60), move || {
        schedule_history_cleanup(ui.clone(), Arc::clone(&storage), Arc::clone(&state));
        refresh_history_after_cleanup(ui, storage, state);
    });
}

fn refresh_history_after_cleanup(
    ui: slint::Weak<AppWindow>,
    storage: Arc<SqliteStorage>,
    state: Arc<Mutex<UiDataState>>,
) {
    spawn_named("saymore-cleanup-history", move || {
        let history_result = storage.cleanup_history(now_ms());
        match history_result {
            Ok(_) => {
                let usage_ui = ui.clone();
                let _ = usage_ui.upgrade_in_event_loop(|ui| ui.invoke_refresh_usage());
                refresh_history_async(ui, storage, state);
            }
            Err(error) => {
                let message = error.to_string();
                let _ = ui.upgrade_in_event_loop(move |ui| {
                    tracing::warn!(event = "history.scheduled_cleanup_failed", reason = %message);
                    ui.set_history_status(ui.global::<Translations>().get_storage_error());
                });
            }
        }
    });
}

pub(super) fn apply_settings(ui: &AppWindow, settings: &template_app::LocalSettings) {
    ui.set_history_enabled(settings.history_enabled);
    ui.set_history_retention(if !settings.history_enabled {
        UiHistoryRetention::Never
    } else {
        match settings.history_retention {
            HistoryRetention::OneDay => UiHistoryRetention::OneDay,
            HistoryRetention::SevenDays => UiHistoryRetention::SevenDays,
            HistoryRetention::ThirtyDays => UiHistoryRetention::ThirtyDays,
            HistoryRetention::Forever => UiHistoryRetention::Forever,
        }
    });
    ui.set_diagnostics_enabled(settings.diagnostics_logging_enabled);
}

fn apply_microphone_devices(
    ui: &AppWindow,
    settings: &template_app::LocalSettings,
    devices: Vec<template_app::AudioInputDevice>,
) {
    let default_name = devices
        .iter()
        .find(|device| device.is_system_default)
        .map(|device| device.name.as_str());
    let default_label = default_name.map_or_else(
        || {
            ui.global::<Translations>()
                .get_microphone_system_default()
                .to_string()
        },
        |name| {
            ui.global::<Translations>()
                .invoke_microphone_system_default_named(name.into())
                .to_string()
        },
    );
    let selected_available = settings
        .preferred_microphone_id
        .as_deref()
        .is_some_and(|id| devices.iter().any(|device| device.id == id));
    let selection_label = settings
        .preferred_microphone_id
        .as_deref()
        .and_then(|id| {
            devices
                .iter()
                .find(|device| device.id == id)
                .map(|device| device.name.clone())
        })
        .or_else(|| {
            settings.preferred_microphone_name.as_deref().map(|name| {
                ui.global::<Translations>()
                    .invoke_microphone_disconnected(name.into())
                    .to_string()
            })
        })
        .unwrap_or_else(|| default_label.clone());
    let devices = devices
        .into_iter()
        .map(|device| UiAudioInputDevice {
            id: SharedString::from(device.id),
            name: SharedString::from(device.name),
        })
        .collect::<Vec<_>>();

    ui.set_microphone_devices(ModelRc::new(VecModel::from(devices)));
    ui.set_microphone_selection_id(SharedString::from(
        settings
            .preferred_microphone_id
            .as_deref()
            .unwrap_or_default(),
    ));
    ui.set_microphone_default_label(SharedString::from(default_label));
    ui.set_microphone_selection_label(SharedString::from(selection_label));
    if settings.preferred_microphone_id.is_none() || selected_available {
        ui.set_microphone_selection_status(SharedString::new());
    }
}
