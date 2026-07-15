use std::sync::{Arc, Mutex};

use chrono::{Local, TimeZone};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use template_app::{HistoryDelivery, HistoryRecord, HistoryRefinement, HistoryStore};
use template_infra::SqliteStorage;

use super::{UiDataState, spawn_named};
use crate::{
    regional_format,
    ui::{AppWindow, HistoryGroup, HistoryListItem, Translations},
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum HistoryGroupKind {
    Today,
    PastWeek,
    PastMonth,
    Older,
}

pub(super) fn refresh_history_async(
    ui: slint::Weak<AppWindow>,
    storage: Arc<SqliteStorage>,
    state: Arc<Mutex<UiDataState>>,
) {
    let (generation, query) = if let Ok(mut state) = state.lock() {
        state.history_generation = state.history_generation.saturating_add(1);
        state.load_more_in_flight = false;
        (state.history_generation, state.history_query.clone())
    } else {
        return;
    };
    if let Some(ui) = ui.upgrade() {
        ui.set_history_loading(true);
        ui.set_history_load_failed(false);
        ui.set_history_status(SharedString::new());
    }
    spawn_named("saymore-load-history", move || {
        let result = storage.search_history_page(None, 50, &query);
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
                    ui.set_history_loading(false);
                    ui.set_history_load_failed(false);
                    ui.set_history_status(SharedString::new());
                }
            }
            Err(error) => {
                if state
                    .lock()
                    .is_ok_and(|state| state.history_generation != generation)
                {
                    return;
                }
                ui.set_history_loading(false);
                apply_history_error(&ui, error);
            }
        });
    });
}

pub(super) fn load_more_history_async(
    ui: slint::Weak<AppWindow>,
    storage: Arc<SqliteStorage>,
    state: Arc<Mutex<UiDataState>>,
) {
    let (cursor, generation, query) = if let Ok(mut state) = state.lock() {
        if state.load_more_in_flight {
            return;
        }
        let Some(cursor) = state.next_history_cursor.clone() else {
            return;
        };
        state.load_more_in_flight = true;
        (
            cursor,
            state.history_generation,
            state.history_query.clone(),
        )
    } else {
        return;
    };
    spawn_named("saymore-load-more-history", move || {
        let result = storage.search_history_page(Some(cursor), 50, &query);
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
                let current = if let Ok(mut state) = state.lock() {
                    let current = state.history_generation == generation;
                    if current {
                        state.load_more_in_flight = false;
                    }
                    current
                } else {
                    false
                };
                if current {
                    apply_history_error(&ui, error);
                }
            }
        });
    });
}

pub(super) fn apply_history_error(ui: &AppWindow, error: template_app::StorageError) {
    let locked = matches!(
        &error,
        template_app::StorageError::HistoryLocked | template_app::StorageError::Invalid(_)
    );
    ui.set_history_locked(locked);
    ui.set_history_load_failed(!locked);
    tracing::warn!(event = "history.load_failed", reason = %error);
    ui.set_history_status(ui.global::<Translations>().get_storage_error());
}

pub(super) fn set_history_model(ui: &AppWindow, state: &UiDataState) {
    let locale = regional_format::date_locale(regional_format::system_locale().as_deref());
    let pending = state
        .pending_history_delete
        .as_ref()
        .map(|(_, id)| id.as_str());
    let visible = state
        .history
        .iter()
        .filter(|record| Some(record.id.as_str()) != pending)
        .collect::<Vec<_>>();
    let mut groups = Vec::new();
    for group in [
        HistoryGroupKind::Today,
        HistoryGroupKind::PastWeek,
        HistoryGroupKind::PastMonth,
        HistoryGroupKind::Older,
    ] {
        let items = visible
            .iter()
            .filter(|record| history_group(record.created_at_ms) == group)
            .map(|record| history_item(ui, record, locale))
            .collect::<Vec<_>>();
        if !items.is_empty() {
            groups.push(HistoryGroup {
                title: history_group_title(ui, group),
                items: ModelRc::new(VecModel::from(items)),
            });
        }
    }
    ui.set_history_groups(ModelRc::new(VecModel::from(groups)));
}

fn history_item(ui: &AppWindow, record: &HistoryRecord, locale: chrono::Locale) -> HistoryListItem {
    let translations = ui.global::<Translations>();
    let delivery = match record.delivery {
        HistoryDelivery::Delivered => translations.get_history_delivered(),
        HistoryDelivery::NotDelivered => translations.get_history_not_delivered(),
    };
    let refinement = match record.refinement {
        HistoryRefinement::Completed => translations.get_history_polished(),
        HistoryRefinement::NotUsed
        | HistoryRefinement::TimedOut
        | HistoryRefinement::ProviderUnavailable
        | HistoryRefinement::OutputRejected => translations.get_history_not_polished(),
    };
    HistoryListItem {
        id: SharedString::from(&record.id),
        text: SharedString::from(&record.final_text),
        time: SharedString::from(history_time(record.created_at_ms, locale)),
        duration: translations.invoke_history_duration(
            i32::try_from(record.audio_duration_ms.div_ceil(1_000)).unwrap_or(i32::MAX),
        ),
        input_status: delivery,
        polish_status: refinement,
        asr_model: record
            .asr_model
            .as_deref()
            .map(SharedString::from)
            .unwrap_or_else(|| translations.get_history_model_not_recorded()),
        llm_model: record
            .llm_model
            .as_deref()
            .map(SharedString::from)
            .unwrap_or_else(|| translations.get_history_model_not_used()),
    }
}

fn history_group(created_at_ms: i64) -> HistoryGroupKind {
    let Some(created) = Local.timestamp_millis_opt(created_at_ms).single() else {
        return HistoryGroupKind::Older;
    };
    let days = Local::now()
        .date_naive()
        .signed_duration_since(created.date_naive())
        .num_days();
    match days {
        i64::MIN..=-1 | 0 => HistoryGroupKind::Today,
        1..=7 => HistoryGroupKind::PastWeek,
        8..=30 => HistoryGroupKind::PastMonth,
        _ => HistoryGroupKind::Older,
    }
}

fn history_group_title(ui: &AppWindow, group: HistoryGroupKind) -> SharedString {
    let translations = ui.global::<Translations>();
    match group {
        HistoryGroupKind::Today => translations.get_history_group_today(),
        HistoryGroupKind::PastWeek => translations.get_history_group_week(),
        HistoryGroupKind::PastMonth => translations.get_history_group_month(),
        HistoryGroupKind::Older => translations.get_history_group_older(),
    }
}

fn history_time(created_at_ms: i64, locale: chrono::Locale) -> String {
    let Some(created) = Local.timestamp_millis_opt(created_at_ms).single() else {
        return created_at_ms.to_string();
    };
    match history_group(created_at_ms) {
        HistoryGroupKind::Today => created.format_localized("%X", locale).to_string(),
        HistoryGroupKind::PastWeek => created.format_localized("%a %X", locale).to_string(),
        HistoryGroupKind::PastMonth | HistoryGroupKind::Older => {
            created.format_localized("%x %X", locale).to_string()
        }
    }
}
