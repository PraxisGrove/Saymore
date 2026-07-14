use std::{
    sync::{Arc, Mutex},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use chrono::{Local, TimeZone};
use slint::{ComponentHandle, ModelRc, SharedString, Timer, VecModel};
use template_app::{
    DictionaryOrigin, DictionaryStore, HistoryCursor, HistoryDelivery, HistoryRecord,
    HistoryRefinement, HistoryRetention, HistoryStore, LocalSettingsStore, NewDictionaryEntry,
};
use template_infra::{DictionaryFiles, SqliteStorage, copy_text_to_clipboard};

use crate::ui::{AppWindow, DictionaryListItem, HistoryListItem};

#[derive(Default)]
struct UiDataState {
    history: Vec<HistoryRecord>,
    next_history_cursor: Option<HistoryCursor>,
    history_generation: u64,
    load_more_in_flight: bool,
    pending_history_delete: Option<(u64, String)>,
    delete_generation: u64,
}

pub fn wire(ui: &AppWindow, storage: Arc<SqliteStorage>) {
    let state = Arc::new(Mutex::new(UiDataState::default()));
    let settings_guard = Arc::new(Mutex::new(()));
    if let Err(error) = storage.cleanup_history(now_ms()) {
        ui.set_history_status(SharedString::from(error.to_string()));
    }
    load_initial(ui, &storage);
    wire_history(ui, Arc::clone(&storage), Arc::clone(&state));
    wire_dictionary(ui, Arc::clone(&storage));
    wire_local_settings(ui, Arc::clone(&storage), settings_guard);
    schedule_history_cleanup(ui.as_weak(), storage, state);
}

fn load_initial(ui: &AppWindow, storage: &SqliteStorage) {
    refresh_dictionary(ui, storage);
    if let Ok(settings) = storage.load_settings() {
        apply_settings(ui, &settings);
    }
}

fn wire_history(ui: &AppWindow, storage: Arc<SqliteStorage>, state: Arc<Mutex<UiDataState>>) {
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
                Err(error) => ui.set_history_status(SharedString::from(error.to_string())),
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
                    ui.set_history_status(SharedString::from("历史已重新初始化"));
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

fn refresh_history_async(
    ui: slint::Weak<AppWindow>,
    storage: Arc<SqliteStorage>,
    state: Arc<Mutex<UiDataState>>,
) {
    let generation = if let Ok(mut state) = state.lock() {
        state.history_generation = state.history_generation.saturating_add(1);
        state.load_more_in_flight = false;
        state.history_generation
    } else {
        return;
    };
    spawn_named("saymore-load-history", move || {
        let result = storage.history_page(None, 50);
        let _ = ui.upgrade_in_event_loop(move |ui| match result {
            Ok(page) => {
                if let Ok(mut state) = state.lock() {
                    if state.history_generation != generation {
                        return;
                    }
                    state.history = page.records;
                    state.next_history_cursor = page.next_cursor;
                    set_history_model(&ui, &state);
                    ui.set_history_has_more(state.next_history_cursor.is_some());
                    ui.set_history_locked(false);
                    ui.set_history_status(SharedString::new());
                }
            }
            Err(error) => apply_history_error(&ui, error),
        });
    });
}

fn load_more_history_async(
    ui: slint::Weak<AppWindow>,
    storage: Arc<SqliteStorage>,
    state: Arc<Mutex<UiDataState>>,
) {
    let (cursor, generation) = if let Ok(mut state) = state.lock() {
        if state.load_more_in_flight {
            return;
        }
        let Some(cursor) = state.next_history_cursor.clone() else {
            return;
        };
        state.load_more_in_flight = true;
        (cursor, state.history_generation)
    } else {
        return;
    };
    spawn_named("saymore-load-more-history", move || {
        let result = storage.history_page(Some(cursor), 50);
        let _ = ui.upgrade_in_event_loop(move |ui| match result {
            Ok(page) => {
                if let Ok(mut state) = state.lock() {
                    if state.history_generation != generation {
                        return;
                    }
                    state.load_more_in_flight = false;
                    state.history.extend(page.records);
                    state.next_history_cursor = page.next_cursor;
                    set_history_model(&ui, &state);
                    ui.set_history_has_more(state.next_history_cursor.is_some());
                    ui.set_history_status(SharedString::new());
                }
            }
            Err(error) => {
                if let Ok(mut state) = state.lock()
                    && state.history_generation == generation
                {
                    state.load_more_in_flight = false;
                }
                apply_history_error(&ui, error);
            }
        });
    });
}

fn apply_history_error(ui: &AppWindow, error: template_app::StorageError) {
    ui.set_history_locked(matches!(
        &error,
        template_app::StorageError::HistoryLocked | template_app::StorageError::Invalid(_)
    ));
    ui.set_history_status(SharedString::from(error.to_string()));
}

fn set_history_model(ui: &AppWindow, state: &UiDataState) {
    let pending = state
        .pending_history_delete
        .as_ref()
        .map(|(_, id)| id.as_str());
    let items = state
        .history
        .iter()
        .filter(|record| Some(record.id.as_str()) != pending)
        .map(history_item)
        .collect::<Vec<_>>();
    ui.set_history_items(ModelRc::new(VecModel::from(items)));
}

fn history_item(record: &HistoryRecord) -> HistoryListItem {
    let time = Local
        .timestamp_millis_opt(record.created_at_ms)
        .single()
        .map(|value| value.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| record.created_at_ms.to_string());
    let delivery = match record.delivery {
        HistoryDelivery::Delivered => "已输入",
        HistoryDelivery::NotDelivered => "未输入到目标应用",
    };
    let refinement = match record.refinement {
        HistoryRefinement::NotUsed => "未使用精炼",
        HistoryRefinement::Completed => "精炼完成",
        HistoryRefinement::TimedOut => "精炼超时",
        HistoryRefinement::ProviderUnavailable => "精炼服务不可用",
        HistoryRefinement::OutputRejected => "精炼结果已拒绝",
    };
    HistoryListItem {
        id: SharedString::from(&record.id),
        text: SharedString::from(&record.final_text),
        raw_asr_text: SharedString::from(record.raw_asr_text.as_deref().unwrap_or_default()),
        has_raw_asr_text: record.raw_asr_text.is_some(),
        refined_text: SharedString::from(record.llm_refined_text.as_deref().unwrap_or_default()),
        has_refined_text: record.llm_refined_text.is_some(),
        time: SharedString::from(time),
        detail: SharedString::from(format!(
            "{:.1} 秒 · {refinement} · {delivery}",
            record.audio_duration_ms as f64 / 1_000.0
        )),
        delivered: record.delivery == HistoryDelivery::Delivered,
    }
}

fn wire_dictionary(ui: &AppWindow, storage: Arc<SqliteStorage>) {
    let refresh_ui = ui.as_weak();
    let refresh_store = Arc::clone(&storage);
    ui.on_refresh_dictionary(move || {
        refresh_dictionary_async(refresh_ui.clone(), Arc::clone(&refresh_store));
    });
    wire_dictionary_entry_actions(ui, Arc::clone(&storage));
    wire_dictionary_file_actions(ui, storage);
}

fn refresh_dictionary_async(ui: slint::Weak<AppWindow>, storage: Arc<SqliteStorage>) {
    spawn_named("saymore-refresh-dictionary", move || {
        let entries = storage.list_dictionary();
        let _ = ui.upgrade_in_event_loop(move |ui| match entries {
            Ok(entries) => set_dictionary_model(&ui, entries),
            Err(error) => ui.set_dictionary_status(SharedString::from(error.to_string())),
        });
    });
}

fn wire_dictionary_entry_actions(ui: &AppWindow, storage: Arc<SqliteStorage>) {
    let add_ui = ui.as_weak();
    let add_store = Arc::clone(&storage);
    ui.on_add_dictionary_word(move |word, language| {
        let ui = add_ui.clone();
        let store = Arc::clone(&add_store);
        spawn_named("saymore-add-dictionary", move || {
            let result = store.upsert_dictionary(
                NewDictionaryEntry {
                    canonical: word.to_string(),
                    language: language.to_string(),
                    variants: Vec::new(),
                    origin: DictionaryOrigin::Manual,
                },
                now_ms(),
            );
            refresh_dictionary_after(
                ui,
                store,
                result
                    .map(|_| "已添加".to_owned())
                    .map_err(|error| error.to_string()),
            );
        });
    });

    let delete_ui = ui.as_weak();
    let delete_store = Arc::clone(&storage);
    ui.on_delete_dictionary_word(move |id| {
        let ui = delete_ui.clone();
        let store = Arc::clone(&delete_store);
        spawn_named("saymore-delete-dictionary", move || {
            let result = store.delete_dictionary(id.as_str());
            refresh_dictionary_after(
                ui,
                store,
                result
                    .map(|()| "已删除".to_owned())
                    .map_err(|error| error.to_string()),
            );
        });
    });
}

fn wire_dictionary_file_actions(ui: &AppWindow, storage: Arc<SqliteStorage>) {
    let import_ui = ui.as_weak();
    let import_store = Arc::clone(&storage);
    ui.on_import_dictionary_csv(move || {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("CSV", &["csv"])
            .pick_file()
        else {
            return;
        };
        let ui = import_ui.clone();
        let store = Arc::clone(&import_store);
        spawn_named("saymore-import-dictionary", move || {
            let dictionary_store: Arc<dyn DictionaryStore> = store.clone();
            let result = DictionaryFiles::new(dictionary_store)
                .import_csv(&path, "zh-Hans", now_ms())
                .map(|report| format!("已导入 {} 个词条", report.added));
            refresh_dictionary_after(ui, store, result.map_err(|error| error.to_string()));
        });
    });
}

fn refresh_dictionary_after(
    ui: slint::Weak<AppWindow>,
    store: Arc<SqliteStorage>,
    result: Result<String, String>,
) {
    let entries = store.list_dictionary();
    let status = match result {
        Ok(message) => message,
        Err(error) => error,
    };
    let _ = ui.upgrade_in_event_loop(move |ui| {
        match entries {
            Ok(entries) => set_dictionary_model(&ui, entries),
            Err(error) => ui.set_dictionary_status(SharedString::from(error.to_string())),
        }
        ui.set_dictionary_status(SharedString::from(status));
    });
}

fn refresh_dictionary(ui: &AppWindow, storage: &SqliteStorage) {
    match storage.list_dictionary() {
        Ok(entries) => set_dictionary_model(ui, entries),
        Err(error) => ui.set_dictionary_status(SharedString::from(error.to_string())),
    }
}

fn set_dictionary_model(ui: &AppWindow, entries: Vec<template_app::DictionaryEntry>) {
    let items = entries
        .into_iter()
        .map(|entry| DictionaryListItem {
            id: SharedString::from(entry.id),
            canonical: SharedString::from(entry.canonical),
            language: SharedString::from(entry.language),
            origin: SharedString::from(match entry.origin {
                DictionaryOrigin::Manual => "手动添加",
                DictionaryOrigin::Automatic => "自动学习",
            }),
        })
        .collect::<Vec<_>>();
    ui.set_dictionary_items(ModelRc::new(VecModel::from(items)));
}

fn wire_local_settings(
    ui: &AppWindow,
    storage: Arc<SqliteStorage>,
    settings_guard: Arc<Mutex<()>>,
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
    ui.on_set_history_retention(move |label| {
        let retention = match label.as_str() {
            "1 天" => HistoryRetention::OneDay,
            "30 天" => HistoryRetention::ThirtyDays,
            "永久" => HistoryRetention::Forever,
            "7 天" => HistoryRetention::SevenDays,
            _ => HistoryRetention::SevenDays,
        };
        update_settings(
            retention_ui.clone(),
            Arc::clone(&retention_store),
            Arc::clone(&retention_guard),
            move |settings| settings.history_retention = retention,
        );
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
            Ok(settings) => apply_settings(&ui, &settings),
            Err(error) => ui.set_history_status(SharedString::from(error)),
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
                    ui.set_history_status(SharedString::from(message));
                });
            }
        }
    });
}

fn apply_settings(ui: &AppWindow, settings: &template_app::LocalSettings) {
    ui.set_history_enabled(settings.history_enabled);
    ui.set_history_retention(SharedString::from(match settings.history_retention {
        HistoryRetention::OneDay => "1 天",
        HistoryRetention::SevenDays => "7 天",
        HistoryRetention::ThirtyDays => "30 天",
        HistoryRetention::Forever => "永久",
    }));
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

fn spawn_named(name: &str, task: impl FnOnce() + Send + 'static) {
    if thread::Builder::new()
        .name(name.to_owned())
        .spawn(task)
        .is_err()
    {
        tracing::error!(event = "local_data.worker_spawn_failed", worker = name);
    }
}
