use std::{
    sync::{Arc, Mutex},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use slint::{ComponentHandle, ModelRc, SharedString, Timer, VecModel};
use template_app::{
    HistoryCursor, HistoryRecord, HistoryRetention, HistoryStore, LocalSettingsChange,
    LocalSettingsStore, MicrophoneSelection,
};
use template_infra::SqliteStorage;

use crate::ui::{
    AppWindow, AudioInputDevice as UiAudioInputDevice,
    HistoryRetentionOption as UiHistoryRetention, Translations,
};
use crate::{RecorderHandle, local_settings_runtime::LocalSettingsHandle};

mod dictionary_ui;
mod history_query;
mod local_settings;

use history_query::{
    apply_history_error, load_more_history_async, refresh_history_async, set_history_model,
};
use local_settings::{
    apply_settings, refresh_microphone_devices_async, schedule_history_cleanup, wire_local_settings,
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
    recorder: RecorderHandle,
    settings: LocalSettingsHandle,
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
    wire_local_settings(
        ui,
        Arc::clone(&storage),
        Arc::clone(&state),
        settings,
        Arc::clone(&recorder),
    );
    refresh_microphone_devices_async(ui.as_weak(), Arc::clone(&storage), recorder);
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
            copy_history_text(&text);
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

#[cfg(target_os = "macos")]
fn copy_history_text(text: &str) {
    let _ = template_infra::copy_text_to_clipboard(text);
}

#[cfg(not(target_os = "macos"))]
fn copy_history_text(_text: &str) {
    tracing::warn!(
        event = "history.copy_unavailable",
        reason = "clipboard integration is not available on this platform yet"
    );
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
