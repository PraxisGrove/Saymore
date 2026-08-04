use std::sync::{Arc, Mutex};

use chrono::{DateTime, Local, TimeZone};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use template_app::{
    HistoryDelivery, HistoryRecord, HistoryRefinement, HistoryStore, PARAFORMER_PROVIDER_ID,
    QWEN3_ASR_PROVIDER_ID, SENSE_VOICE_PROVIDER_ID, WHISPER_PROVIDER_ID,
};
use template_infra::{
    PARAFORMER_MODEL_ID, QWEN3_ASR_MODEL_ID, SENSE_VOICE_MODEL_ID, SqliteStorage, WHISPER_MODEL_ID,
};

use super::{UiDataState, spawn_named};
use crate::{
    regional_format,
    ui::{AppWindow, HistoryGroup, HistoryListItem, Translations},
};

const MACOS_SPEECH_PROVIDER_ID: &str = "macos-speech";
const MACOS_DICTATION_MODEL_ID: &str = "macos-dictation";
const PARAFORMER_LABEL: &str = "Paraformer";
const QWEN3_ASR_LABEL: &str = "Qwen3-ASR 1.7B";
const SENSE_VOICE_LABEL: &str = "SenseVoiceSmall";
const WHISPER_FULL_LABEL: &str = "Whisper large-v3-turbo";
const VOLCENGINE_ASR_1_MODEL: &str = "volc.bigasr.sauc.duration";
const VOLCENGINE_ASR_2_MODEL: &str = "volc.seedasr.sauc.duration";
const VOLCENGINE_LEGACY_MODEL: &str = "bigmodel_async";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HistoryAsrModel<'a> {
    Named(&'a str),
    MacOsDictation,
    Whisper,
    VolcengineV1,
    VolcengineV2,
}

struct LocalizedHistoryAsrModel {
    short: SharedString,
    full: SharedString,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HistoryGroupKind {
    LastDay,
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
        HistoryGroupKind::LastDay,
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
    let asr_model = localized_history_asr_model(
        &translations,
        record.asr_provider_id.as_deref(),
        record.asr_model.as_deref(),
    );
    let missing_asr_model = translations.get_history_model_not_recorded();
    let (asr_model, asr_model_full) = asr_model
        .map(|model| (model.short, model.full))
        .unwrap_or_else(|| (missing_asr_model.clone(), missing_asr_model));
    let llm_model = record
        .llm_model
        .as_deref()
        .map(SharedString::from)
        .unwrap_or_else(|| translations.get_history_model_not_used());
    HistoryListItem {
        id: SharedString::from(&record.id),
        text: SharedString::from(&record.final_text),
        raw_text: record
            .raw_asr_text
            .as_deref()
            .map(SharedString::from)
            .unwrap_or_default(),
        llm_text: record
            .llm_refined_text
            .as_deref()
            .map(SharedString::from)
            .unwrap_or_default(),
        preview_text: SharedString::from(history_preview(&record.final_text)),
        time: SharedString::from(history_time(record.created_at_ms, locale)),
        duration: translations.invoke_history_duration(
            i32::try_from(record.audio_duration_ms.div_ceil(1_000)).unwrap_or(i32::MAX),
        ),
        input_status: delivery,
        polish_status: refinement,
        asr_model,
        asr_model_full,
        llm_model: llm_model.clone(),
        llm_model_full: llm_model,
    }
}

fn localized_history_asr_model(
    translations: &Translations,
    provider_id: Option<&str>,
    model: Option<&str>,
) -> Option<LocalizedHistoryAsrModel> {
    match history_asr_model(provider_id, model)? {
        HistoryAsrModel::Named(name) => Some(same_history_asr_model(name)),
        HistoryAsrModel::MacOsDictation => Some(same_history_asr_model(
            translations.get_history_model_macos_dictation(),
        )),
        HistoryAsrModel::Whisper => Some(LocalizedHistoryAsrModel {
            short: translations.get_history_model_whisper_turbo(),
            full: SharedString::from(WHISPER_FULL_LABEL),
        }),
        HistoryAsrModel::VolcengineV1 => Some(LocalizedHistoryAsrModel {
            short: translations.get_history_model_volcengine_v1_short(),
            full: translations.get_history_model_volcengine_v1(),
        }),
        HistoryAsrModel::VolcengineV2 => Some(LocalizedHistoryAsrModel {
            short: translations.get_history_model_volcengine_v2_short(),
            full: translations.get_history_model_volcengine_v2(),
        }),
    }
}

fn same_history_asr_model(value: impl Into<SharedString>) -> LocalizedHistoryAsrModel {
    let value = value.into();
    LocalizedHistoryAsrModel {
        short: value.clone(),
        full: value,
    }
}

fn history_asr_model<'a>(
    provider_id: Option<&str>,
    model: Option<&'a str>,
) -> Option<HistoryAsrModel<'a>> {
    match model {
        Some(MACOS_DICTATION_MODEL_ID) => Some(HistoryAsrModel::MacOsDictation),
        Some(PARAFORMER_MODEL_ID) => Some(HistoryAsrModel::Named(PARAFORMER_LABEL)),
        Some(QWEN3_ASR_MODEL_ID) => Some(HistoryAsrModel::Named(QWEN3_ASR_LABEL)),
        Some(SENSE_VOICE_MODEL_ID) => Some(HistoryAsrModel::Named(SENSE_VOICE_LABEL)),
        Some(WHISPER_MODEL_ID) => Some(HistoryAsrModel::Whisper),
        Some(VOLCENGINE_ASR_1_MODEL) => Some(HistoryAsrModel::VolcengineV1),
        Some(VOLCENGINE_ASR_2_MODEL | VOLCENGINE_LEGACY_MODEL) => {
            Some(HistoryAsrModel::VolcengineV2)
        }
        Some(model) => Some(HistoryAsrModel::Named(model)),
        None => match provider_id {
            Some(MACOS_SPEECH_PROVIDER_ID) => Some(HistoryAsrModel::MacOsDictation),
            Some(PARAFORMER_PROVIDER_ID) => Some(HistoryAsrModel::Named(PARAFORMER_LABEL)),
            Some(QWEN3_ASR_PROVIDER_ID) => Some(HistoryAsrModel::Named(QWEN3_ASR_LABEL)),
            Some(SENSE_VOICE_PROVIDER_ID) => Some(HistoryAsrModel::Named(SENSE_VOICE_LABEL)),
            Some(WHISPER_PROVIDER_ID) => Some(HistoryAsrModel::Whisper),
            _ => None,
        },
    }
}

fn history_preview(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn history_group(created_at_ms: i64) -> HistoryGroupKind {
    history_group_at(created_at_ms, Local::now())
}

fn history_group_at(created_at_ms: i64, now: DateTime<Local>) -> HistoryGroupKind {
    let Some(created) = Local.timestamp_millis_opt(created_at_ms).single() else {
        return HistoryGroupKind::Older;
    };
    let elapsed_seconds = now.signed_duration_since(created).num_seconds();
    match elapsed_seconds {
        i64::MIN..86_400 => HistoryGroupKind::LastDay,
        86_400..604_800 => HistoryGroupKind::PastWeek,
        604_800..2_592_000 => HistoryGroupKind::PastMonth,
        _ => HistoryGroupKind::Older,
    }
}

fn history_group_title(ui: &AppWindow, group: HistoryGroupKind) -> SharedString {
    let translations = ui.global::<Translations>();
    match group {
        HistoryGroupKind::LastDay => translations.get_history_group_last_day(),
        HistoryGroupKind::PastWeek => translations.get_history_group_week(),
        HistoryGroupKind::PastMonth => translations.get_history_group_month(),
        HistoryGroupKind::Older => translations.get_history_group_older(),
    }
}

fn history_time(created_at_ms: i64, locale: chrono::Locale) -> String {
    let Some(created) = Local.timestamp_millis_opt(created_at_ms).single() else {
        return created_at_ms.to_string();
    };
    history_time_for_group(created, history_group(created_at_ms), locale)
}

fn history_time_for_group(
    created: DateTime<Local>,
    group: HistoryGroupKind,
    locale: chrono::Locale,
) -> String {
    match group {
        HistoryGroupKind::LastDay => created.format("%H:%M:%S").to_string(),
        HistoryGroupKind::PastWeek => created.format_localized("%A", locale).to_string(),
        HistoryGroupKind::PastMonth | HistoryGroupKind::Older => {
            created.format("%m-%d").to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local_time(
        year: i32,
        month: u32,
        day: u32,
        hour: u32,
        minute: u32,
        second: u32,
    ) -> DateTime<Local> {
        match Local
            .with_ymd_and_hms(year, month, day, hour, minute, second)
            .single()
        {
            Some(value) => value,
            None => panic!("test date should be valid in the local time zone"),
        }
    }

    #[test]
    fn history_time_uses_compact_fields_for_each_age_group() {
        let now = local_time(2026, 7, 16, 1, 0, 0);
        let today = local_time(2026, 7, 16, 0, 8, 37);
        let this_week = local_time(2026, 7, 14, 23, 17, 32);
        let this_month = local_time(2026, 7, 1, 23, 17, 32);
        let older = local_time(2026, 6, 15, 23, 17, 32);

        assert_eq!(
            HistoryGroupKind::LastDay,
            history_group_at(today.timestamp_millis(), now)
        );
        assert_eq!(
            HistoryGroupKind::PastWeek,
            history_group_at(this_week.timestamp_millis(), now)
        );
        assert_eq!(
            HistoryGroupKind::PastMonth,
            history_group_at(this_month.timestamp_millis(), now)
        );
        assert_eq!(
            HistoryGroupKind::Older,
            history_group_at(older.timestamp_millis(), now)
        );

        assert_eq!(
            "00:08:37",
            history_time_for_group(today, HistoryGroupKind::LastDay, chrono::Locale::zh_CN)
        );
        assert_eq!(
            "星期二",
            history_time_for_group(this_week, HistoryGroupKind::PastWeek, chrono::Locale::zh_CN)
        );
        assert_eq!(
            "07-01",
            history_time_for_group(
                this_month,
                HistoryGroupKind::PastMonth,
                chrono::Locale::zh_CN
            )
        );
        assert_eq!(
            "06-15",
            history_time_for_group(older, HistoryGroupKind::Older, chrono::Locale::zh_CN)
        );
    }

    #[test]
    fn history_preview_collapses_formatted_text_to_one_line() {
        assert_eq!(
            "I want three things: 1. Large. 2. Small. 3. Long.",
            history_preview("I want three things:\n\n1. Large.\n2. Small.\r\n3. Long.")
        );
    }

    #[test]
    fn history_asr_model_uses_known_provider_names_when_no_model_exists() {
        assert_eq!(
            Some(HistoryAsrModel::MacOsDictation),
            history_asr_model(Some(MACOS_SPEECH_PROVIDER_ID), None)
        );
        assert_eq!(
            Some(HistoryAsrModel::Named(PARAFORMER_LABEL)),
            history_asr_model(Some(PARAFORMER_PROVIDER_ID), None)
        );
        assert_eq!(
            Some(HistoryAsrModel::Whisper),
            history_asr_model(Some(WHISPER_PROVIDER_ID), None)
        );
        assert_eq!(
            Some(HistoryAsrModel::Named(QWEN3_ASR_LABEL)),
            history_asr_model(Some(QWEN3_ASR_PROVIDER_ID), None)
        );
        assert_eq!(
            Some(HistoryAsrModel::Named(SENSE_VOICE_LABEL)),
            history_asr_model(Some(SENSE_VOICE_PROVIDER_ID), None)
        );
        assert_eq!(None, history_asr_model(None, None));
    }

    #[test]
    fn history_asr_model_converts_built_in_model_ids_to_product_names() {
        assert_eq!(
            Some(HistoryAsrModel::MacOsDictation),
            history_asr_model(None, Some(MACOS_DICTATION_MODEL_ID))
        );
        assert_eq!(
            Some(HistoryAsrModel::Named(PARAFORMER_LABEL)),
            history_asr_model(None, Some(PARAFORMER_MODEL_ID))
        );
        assert_eq!(
            Some(HistoryAsrModel::Whisper),
            history_asr_model(None, Some(WHISPER_MODEL_ID))
        );
        assert_eq!(
            Some(HistoryAsrModel::Named(QWEN3_ASR_LABEL)),
            history_asr_model(None, Some(QWEN3_ASR_MODEL_ID))
        );
        assert_eq!(
            Some(HistoryAsrModel::Named(SENSE_VOICE_LABEL)),
            history_asr_model(None, Some(SENSE_VOICE_MODEL_ID))
        );
    }

    #[test]
    fn history_asr_model_converts_volcengine_resource_ids_to_product_names() {
        assert_eq!(
            Some(HistoryAsrModel::VolcengineV1),
            history_asr_model(Some("volcengine"), Some(VOLCENGINE_ASR_1_MODEL))
        );
        for model in [VOLCENGINE_ASR_2_MODEL, VOLCENGINE_LEGACY_MODEL] {
            assert_eq!(
                Some(HistoryAsrModel::VolcengineV2),
                history_asr_model(Some("volcengine"), Some(model))
            );
        }
    }

    #[test]
    fn history_asr_model_preserves_custom_model_names() {
        assert_eq!(
            Some(HistoryAsrModel::Named("custom-asr-model")),
            history_asr_model(Some("custom-asr"), Some("custom-asr-model"))
        );
    }
}
