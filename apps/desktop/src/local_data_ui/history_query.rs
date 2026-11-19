use std::sync::{Arc, Mutex};

use jiff::{Timestamp, civil::DateTime};
use pure_rust_locales::Locale;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use template_app::{
    HistoryDelivery, HistoryRecord, HistoryRefinement, HistoryStore, PARAFORMER_PROVIDER_ID,
    QWEN3_ASR_PROVIDER_ID, SENSE_VOICE_PROVIDER_ID, WHISPER_PROVIDER_ID,
};
use template_infra::{
    PARAFORMER_MODEL_ID, QWEN3_ASR_MODEL_ID, RetiredAsrModel, SqliteStorage, WHISPER_MODEL_ID,
    system_local_datetime,
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
    let now = Timestamp::now();
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
            .filter(|record| history_group_at(record.created_at_ms, now) == group)
            .map(|record| {
                let time = history_time(record.created_at_ms, group, locale, system_local_datetime);
                history_item(ui, record, time)
            })
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

fn history_item(ui: &AppWindow, record: &HistoryRecord, time: String) -> HistoryListItem {
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
        time: time.into(),
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
        Some(model) if model == RetiredAsrModel::SenseVoice.id() => {
            Some(HistoryAsrModel::Named(SENSE_VOICE_LABEL))
        }
        Some(template_infra::SEACO_MODEL_ID) => Some(HistoryAsrModel::Named("SeACo-Paraformer")),
        Some(template_infra::ZIPFORMER_MODEL_ID) => Some(HistoryAsrModel::Named("Zipformer")),
        Some(model) if model == RetiredAsrModel::Moonshine.id() => {
            Some(HistoryAsrModel::Named("Moonshine"))
        }
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
            Some(template_app::SEACO_PROVIDER_ID) => {
                Some(HistoryAsrModel::Named("SeACo-Paraformer"))
            }
            Some(template_app::ZIPFORMER_PROVIDER_ID) => Some(HistoryAsrModel::Named("Zipformer")),
            Some(template_app::MOONSHINE_PROVIDER_ID) => Some(HistoryAsrModel::Named("Moonshine")),
            Some(WHISPER_PROVIDER_ID) => Some(HistoryAsrModel::Whisper),
            _ => None,
        },
    }
}

fn history_preview(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn history_group_at(created_at_ms: i64, now: Timestamp) -> HistoryGroupKind {
    let Ok(created) = Timestamp::from_millisecond(created_at_ms) else {
        return HistoryGroupKind::Older;
    };
    let elapsed_seconds = now.duration_since(created).as_secs();
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

fn history_time(
    created_at_ms: i64,
    group: HistoryGroupKind,
    locale: Locale,
    to_local: impl FnOnce(Timestamp) -> DateTime,
) -> String {
    let Ok(created) = Timestamp::from_millisecond(created_at_ms) else {
        return created_at_ms.to_string();
    };
    let local = to_local(created);
    match group {
        HistoryGroupKind::LastDay => local.strftime("%H:%M:%S").to_string(),
        HistoryGroupKind::PastWeek => {
            regional_format::weekday_name(local.date(), locale).to_owned()
        }
        HistoryGroupKind::PastMonth | HistoryGroupKind::Older => {
            local.strftime("%m-%d").to_string()
        }
    }
}

#[cfg(test)]
#[allow(clippy::panic_in_result_fn)]
mod tests {
    use super::*;
    use jiff::tz::TimeZone;

    #[test]
    fn history_time_uses_compact_fields_for_each_age_group() -> Result<(), jiff::Error> {
        let now: Timestamp = "2026-07-16T01:00:00Z".parse()?;
        for (recorded, expected_group, expected_label) in [
            (
                "2026-07-16T00:08:37Z",
                HistoryGroupKind::LastDay,
                "00:08:37",
            ),
            ("2026-07-14T23:17:32Z", HistoryGroupKind::PastWeek, "星期二"),
            ("2026-07-01T23:17:32Z", HistoryGroupKind::PastMonth, "07-01"),
            ("2026-06-15T23:17:32Z", HistoryGroupKind::Older, "06-15"),
        ] {
            let millis = recorded.parse::<Timestamp>()?.as_millisecond();
            let group = history_group_at(millis, now);
            assert_eq!(expected_group, group);
            assert_eq!(
                expected_label,
                history_time(millis, group, Locale::zh_CN, |time| TimeZone::UTC
                    .to_datetime(time))
            );
        }
        Ok(())
    }

    #[test]
    fn history_uses_elapsed_hours_and_local_dates_across_dst() -> Result<(), jiff::Error> {
        let zone = TimeZone::get("America/New_York")?;
        // Midnight to midnight spans only 23 hours when daylight saving time starts.
        let now: Timestamp = "2026-03-09T00:00:00-04:00".parse()?;
        let yesterday = "2026-03-08T00:00:00-05:00"
            .parse::<Timestamp>()?
            .as_millisecond();
        assert_eq!(HistoryGroupKind::LastDay, history_group_at(yesterday, now));
        assert_eq!(
            "00:00:00",
            history_time(
                yesterday,
                HistoryGroupKind::LastDay,
                Locale::en_US,
                |time| zone.to_datetime(time)
            )
        );
        // Exactly 24 hours ago is still Saturday locally, although already Sunday in UTC.
        let boundary = "2026-03-08T04:00:00Z"
            .parse::<Timestamp>()?
            .as_millisecond();
        assert_eq!(HistoryGroupKind::PastWeek, history_group_at(boundary, now));
        assert_eq!(
            "Saturday",
            history_time(
                boundary,
                HistoryGroupKind::PastWeek,
                Locale::en_US,
                |time| zone.to_datetime(time)
            )
        );
        Ok(())
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
            history_asr_model(None, Some(RetiredAsrModel::SenseVoice.id()))
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
