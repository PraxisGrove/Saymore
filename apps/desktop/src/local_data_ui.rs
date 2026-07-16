use std::{
    sync::{Arc, Mutex},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use slint::{ComponentHandle, ModelRc, SharedString, Timer, VecModel};
use template_app::{
    HistoryCursor, HistoryRecord, HistoryRetention, HistoryStore, LocalSettingsStore,
};
use template_infra::{MacOsAudioRecorder, SqliteStorage, copy_text_to_clipboard};

use crate::ui::{
    AppWindow, AudioInputDevice as UiAudioInputDevice,
    HistoryRetentionOption as UiHistoryRetention, Translations,
};

mod dictionary_ui;
mod history_query;

use history_query::{
    apply_history_error, load_more_history_async, refresh_history_async, set_history_model,
};

#[derive(Default)]
struct UiDataState {
    history: Vec<HistoryRecord>,
    next_history_cursor: Option<HistoryCursor>,
    history_generation: u64,
    load_more_in_flight: bool,
    pending_history_delete: Option<(u64, String)>,
    delete_generation: u64,
    history_query: String,
}

pub fn wire(
    ui: &AppWindow,
    storage: Arc<SqliteStorage>,
    recorder: Arc<Mutex<MacOsAudioRecorder>>,
    settings_guard: Arc<Mutex<()>>,
) {
    let state = Arc::new(Mutex::new(UiDataState::default()));
    let dictionary_state = Arc::new(Mutex::new(dictionary_ui::DictionaryUiState::default()));
    if let Err(error) = storage.cleanup_history(now_ms()) {
        tracing::warn!(event = "history.cleanup_failed", reason = %error);
        ui.set_history_status(ui.global::<Translations>().get_storage_error());
    }
    load_initial(ui, &storage, &dictionary_state);
    wire_history(ui, Arc::clone(&storage), Arc::clone(&state));
    dictionary_ui::wire(ui, Arc::clone(&storage), dictionary_state);
    wire_local_settings(ui, Arc::clone(&storage), settings_guard, recorder);
    refresh_microphone_devices_async(ui.as_weak(), Arc::clone(&storage));
    schedule_history_cleanup(ui.as_weak(), storage, state);
}

fn load_initial(
    ui: &AppWindow,
    storage: &SqliteStorage,
    dictionary_state: &Arc<Mutex<dictionary_ui::DictionaryUiState>>,
) {
    dictionary_ui::load_initial(ui, storage, dictionary_state);
    if let Ok(settings) = storage.load_settings() {
        apply_settings(ui, &settings);
    }
}

fn wire_history(ui: &AppWindow, storage: Arc<SqliteStorage>, state: Arc<Mutex<UiDataState>>) {
    wire_history_queries(ui, Arc::clone(&storage), Arc::clone(&state));
    wire_history_item_actions(ui, Arc::clone(&storage), Arc::clone(&state));
    wire_history_bulk_actions(ui, storage, state);
}

fn wire_history_queries(
    ui: &AppWindow,
    storage: Arc<SqliteStorage>,
    state: Arc<Mutex<UiDataState>>,
) {
    let refresh_ui = ui.as_weak();
    let refresh_store = Arc::clone(&storage);
    let refresh_state = Arc::clone(&state);
    ui.on_refresh_history(move || {
        refresh_history_async(
            refresh_ui.clone(),
            Arc::clone(&refresh_store),
            Arc::clone(&refresh_state),
        );
    });

    let search_ui = ui.as_weak();
    let search_store = Arc::clone(&storage);
    let search_state = Arc::clone(&state);
    ui.on_search_history(move |query| {
        if let Ok(mut state) = search_state.lock() {
            state.history_query = query.to_string();
        }
        refresh_history_async(
            search_ui.clone(),
            Arc::clone(&search_store),
            Arc::clone(&search_state),
        );
    });

    let load_more_ui = ui.as_weak();
    let load_more_store = Arc::clone(&storage);
    let load_more_state = Arc::clone(&state);
    ui.on_load_more_history(move || {
        load_more_history_async(
            load_more_ui.clone(),
            Arc::clone(&load_more_store),
            Arc::clone(&load_more_state),
        );
    });
}

fn wire_history_item_actions(
    ui: &AppWindow,
    storage: Arc<SqliteStorage>,
    state: Arc<Mutex<UiDataState>>,
) {
    let copy_state = Arc::clone(&state);
    ui.on_copy_history(move |id| {
        let text = copy_state.lock().ok().and_then(|state| {
            state
                .history
                .iter()
                .find(|record| record.id == id.as_str())
                .map(|record| record.final_text.clone())
        });
        if let Some(text) = text {
            let _ = copy_text_to_clipboard(&text);
        }
    });

    let delete_ui = ui.as_weak();
    let delete_store = Arc::clone(&storage);
    let delete_state = Arc::clone(&state);
    ui.on_delete_history(move |id| {
        schedule_history_delete(
            &delete_ui,
            Arc::clone(&delete_store),
            Arc::clone(&delete_state),
            id.to_string(),
        );
    });

    let undo_ui = ui.as_weak();
    let undo_state = Arc::clone(&state);
    ui.on_undo_history_delete(move || {
        if let Ok(mut state) = undo_state.lock() {
            state.pending_history_delete = None;
            state.delete_generation = state.delete_generation.saturating_add(1);
            if let Some(ui) = undo_ui.upgrade() {
                ui.set_history_undo_visible(false);
                set_history_model(&ui, &state);
            }
        }
    });
}

fn wire_history_bulk_actions(
    ui: &AppWindow,
    storage: Arc<SqliteStorage>,
    state: Arc<Mutex<UiDataState>>,
) {
    let clear_ui = ui.as_weak();
    let clear_store = Arc::clone(&storage);
    let clear_state = Arc::clone(&state);
    ui.on_clear_history(move || {
        let store = Arc::clone(&clear_store);
        let ui = clear_ui.clone();
        let state = Arc::clone(&clear_state);
        spawn_named("saymore-clear-history", move || {
            let result = store.clear_history();
            let _ = ui.upgrade_in_event_loop(move |ui| match result {
                Ok(()) => {
                    if let Ok(mut state) = state.lock() {
                        state.history.clear();
                        state.next_history_cursor = None;
                        state.pending_history_delete = None;
                        ui.set_history_undo_visible(false);
                        ui.set_history_has_more(false);
                        set_history_model(&ui, &state);
                    }
                    ui.invoke_refresh_usage();
                }
                Err(error) => {
                    tracing::warn!(event = "history.clear_failed", reason = %error);
                    ui.set_history_status(ui.global::<Translations>().get_storage_error());
                }
            });
        });
    });

    let reset_ui = ui.as_weak();
    let reset_store = Arc::clone(&storage);
    let reset_state = Arc::clone(&state);
    ui.on_reset_history(move || {
        let ui = reset_ui.clone();
        let store = Arc::clone(&reset_store);
        let state = Arc::clone(&reset_state);
        spawn_named("saymore-reset-history", move || {
            let result = store.reset_history();
            let _ = ui.upgrade_in_event_loop(move |ui| match result {
                Ok(()) => {
                    if let Ok(mut state) = state.lock() {
                        state.history.clear();
                        state.next_history_cursor = None;
                        state.pending_history_delete = None;
                        set_history_model(&ui, &state);
                    }
                    ui.set_history_locked(false);
                    ui.set_history_has_more(false);
                    ui.set_history_undo_visible(false);
                    ui.set_history_status(ui.global::<Translations>().get_history_reset_complete());
                    ui.invoke_refresh_usage();
                }
                Err(error) => apply_history_error(&ui, error),
            });
        });
    });
}

fn schedule_history_delete(
    ui: &slint::Weak<AppWindow>,
    storage: Arc<SqliteStorage>,
    state: Arc<Mutex<UiDataState>>,
    id: String,
) {
    let previous = if let Ok(mut state) = state.lock() {
        let previous = state.pending_history_delete.take().map(|(_, id)| id);
        if let Some(previous) = &previous {
            state.history.retain(|record| &record.id != previous);
        }
        state.delete_generation = state.delete_generation.saturating_add(1);
        let generation = state.delete_generation;
        state.pending_history_delete = Some((generation, id.clone()));
        if let Some(ui) = ui.upgrade() {
            ui.set_history_undo_visible(true);
            set_history_model(&ui, &state);
        }
        previous
    } else {
        None
    };
    if let Some(previous) = previous {
        commit_history_delete(
            ui.clone(),
            Arc::clone(&storage),
            Arc::clone(&state),
            previous,
        );
    }
    let timer_ui = ui.clone();
    Timer::single_shot(Duration::from_secs(3), move || {
        let pending = state.lock().ok().and_then(|mut state| {
            let matches = state
                .pending_history_delete
                .as_ref()
                .is_some_and(|(_, pending_id)| pending_id == &id);
            matches
                .then(|| state.pending_history_delete.take())
                .flatten()
        });
        if let Some((_, pending_id)) = pending {
            if let Some(ui) = timer_ui.upgrade() {
                ui.set_history_undo_visible(false);
                if let Ok(mut state) = state.lock() {
                    state.history.retain(|record| record.id != pending_id);
                    set_history_model(&ui, &state);
                }
            }
            commit_history_delete(timer_ui, storage, state, pending_id);
        }
    });
}

fn commit_history_delete(
    ui: slint::Weak<AppWindow>,
    storage: Arc<SqliteStorage>,
    state: Arc<Mutex<UiDataState>>,
    id: String,
) {
    spawn_named("saymore-delete-history", move || {
        let result = storage.delete_history(&id);
        let _ = ui.upgrade_in_event_loop(move |ui| match result {
            Ok(()) => ui.invoke_refresh_usage(),
            Err(error) => {
                apply_history_error(&ui, error);
                refresh_history_async(ui.as_weak(), storage, state);
            }
        });
    });
}

fn wire_local_settings(
    ui: &AppWindow,
    storage: Arc<SqliteStorage>,
    settings_guard: Arc<Mutex<()>>,
    recorder: Arc<Mutex<MacOsAudioRecorder>>,
) {
    let history_ui = ui.as_weak();
    let history_store = Arc::clone(&storage);
    let history_guard = Arc::clone(&settings_guard);
    ui.on_set_history_enabled(move |enabled| {
        update_settings(
            history_ui.clone(),
            Arc::clone(&history_store),
            Arc::clone(&history_guard),
            move |settings| settings.history_enabled = enabled,
        );
    });
    let retention_ui = ui.as_weak();
    let retention_store = Arc::clone(&storage);
    let retention_guard = Arc::clone(&settings_guard);
    ui.on_set_history_retention(move |selection| {
        let (enabled, retention) = match selection {
            UiHistoryRetention::Never => (false, HistoryRetention::SevenDays),
            UiHistoryRetention::OneDay => (true, HistoryRetention::OneDay),
            UiHistoryRetention::SevenDays => (true, HistoryRetention::SevenDays),
            UiHistoryRetention::ThirtyDays => (true, HistoryRetention::ThirtyDays),
            UiHistoryRetention::Forever => (true, HistoryRetention::Forever),
        };
        update_settings(
            retention_ui.clone(),
            Arc::clone(&retention_store),
            Arc::clone(&retention_guard),
            move |settings| {
                settings.history_enabled = enabled;
                settings.history_retention = retention;
            },
        );
    });

    let microphone_ui = ui.as_weak();
    let microphone_store = Arc::clone(&storage);
    let microphone_guard = Arc::clone(&settings_guard);
    let microphone_recorder = Arc::clone(&recorder);
    ui.on_select_microphone(move |id, name| {
        save_microphone_selection_async(
            microphone_ui.clone(),
            Arc::clone(&microphone_store),
            Arc::clone(&microphone_guard),
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
    guard: Arc<Mutex<()>>,
    recorder: Arc<Mutex<MacOsAudioRecorder>>,
    preferred_id: Option<String>,
    preferred_name: Option<String>,
) {
    spawn_named("saymore-save-microphone-selection", move || {
        let saved_id = preferred_id.clone();
        let result = guard
            .lock()
            .map_err(|_| "settings update lock was poisoned".to_owned())
            .and_then(|_guard| {
                let mut settings = storage.load_settings().map_err(|error| error.to_string())?;
                settings.preferred_microphone_id = preferred_id;
                settings.preferred_microphone_name = preferred_name;
                storage
                    .save_settings(settings.clone())
                    .map(|()| settings)
                    .map_err(|error| error.to_string())
            });
        if result.is_ok() {
            if let Ok(mut recorder) = recorder.lock() {
                recorder.set_preferred_input_device_id(saved_id);
            } else {
                tracing::error!(
                    event = "microphone.selection_update_failed",
                    reason = "recorder lock was poisoned"
                );
            }
        }
        let refresh_ui = ui.clone();
        let refresh_storage = Arc::clone(&storage);
        let _ = ui.upgrade_in_event_loop(move |ui| match result {
            Ok(settings) => {
                apply_settings(&ui, &settings);
                apply_microphone_devices(&ui, &settings, Vec::new());
                ui.set_microphone_selection_status(SharedString::new());
                refresh_microphone_devices_async(refresh_ui, refresh_storage);
            }
            Err(error) => {
                tracing::warn!(event = "microphone.selection_save_failed", reason = %error);
                ui.set_microphone_selection_status(
                    ui.global::<Translations>().get_settings_save_failed(),
                );
            }
        });
    });
}

fn refresh_microphone_devices_async(ui: slint::Weak<AppWindow>, storage: Arc<SqliteStorage>) {
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

fn update_settings(
    ui: slint::Weak<AppWindow>,
    storage: Arc<SqliteStorage>,
    guard: Arc<Mutex<()>>,
    change: impl FnOnce(&mut template_app::LocalSettings) + Send + 'static,
) {
    spawn_named("saymore-save-local-settings", move || {
        let result = guard
            .lock()
            .map_err(|_| "settings update lock was poisoned".to_owned())
            .and_then(|_guard| {
                let mut settings = storage.load_settings().map_err(|error| error.to_string())?;
                change(&mut settings);
                storage
                    .save_settings(settings.clone())
                    .map(|()| settings)
                    .map_err(|error| error.to_string())
            });
        let _ = ui.upgrade_in_event_loop(move |ui| match result {
            Ok(settings) => {
                apply_settings(&ui, &settings);
                ui.invoke_refresh_history();
            }
            Err(error) => {
                tracing::warn!(event = "history.settings_save_failed", reason = %error);
                ui.set_history_status(ui.global::<Translations>().get_settings_save_failed());
            }
        });
    });
}

fn schedule_history_cleanup(
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
        let now = now_ms();
        let history_result = storage.cleanup_history(now);
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

fn apply_settings(ui: &AppWindow, settings: &template_app::LocalSettings) {
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

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

pub(super) fn spawn_named(name: &str, task: impl FnOnce() + Send + 'static) {
    if thread::Builder::new()
        .name(name.to_owned())
        .spawn(task)
        .is_err()
    {
        tracing::error!(event = "local_data.worker_spawn_failed", worker = name);
    }
}
