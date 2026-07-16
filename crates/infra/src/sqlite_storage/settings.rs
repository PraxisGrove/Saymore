use rusqlite::{Connection, OptionalExtension, params};
use template_app::{
    HistoryRetention, LocalSettings, OnboardingStatus, OnboardingStep, StorageError,
    UiLanguagePreference,
};

use super::unavailable;

pub(super) fn load(connection: &Connection) -> Result<LocalSettings, StorageError> {
    connection
        .query_row(
            "SELECT history_enabled, history_retention_days,
                    preferred_microphone_id, preferred_microphone_name,
                    diagnostics_logging_enabled, ui_language,
                    automatic_update_checks, feedback_sounds_enabled,
                    copy_to_clipboard, show_in_dock, dictation_paused,
                    dictation_shortcuts, onboarding_status, onboarding_step
             FROM app_settings WHERE singleton = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, bool>(0)?,
                    row.get::<_, Option<u16>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, bool>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, bool>(6)?,
                    row.get::<_, bool>(7)?,
                    row.get::<_, bool>(8)?,
                    row.get::<_, bool>(9)?,
                    row.get::<_, bool>(10)?,
                    row.get::<_, String>(11)?,
                    row.get::<_, String>(12)?,
                    row.get::<_, u8>(13)?,
                ))
            },
        )
        .optional()
        .map_err(unavailable)?
        .ok_or_else(|| StorageError::Invalid("app settings row is missing".to_owned()))
        .and_then(
            |(
                history_enabled,
                retention_days,
                microphone_id,
                microphone_name,
                diagnostics_logging_enabled,
                ui_language,
                automatic_update_checks,
                feedback_sounds_enabled,
                copy_to_clipboard,
                show_in_dock,
                dictation_paused,
                dictation_shortcuts,
                onboarding_status,
                onboarding_step,
            )| {
                Ok(LocalSettings {
                    history_enabled,
                    history_retention: retention_from_days(retention_days)?,
                    preferred_microphone_id: microphone_id,
                    preferred_microphone_name: microphone_name,
                    diagnostics_logging_enabled,
                    ui_language: UiLanguagePreference::from_storage_value(&ui_language)
                        .ok_or_else(|| {
                            StorageError::Invalid(format!(
                                "unsupported UI language preference: {ui_language}"
                            ))
                        })?,
                    automatic_update_checks,
                    feedback_sounds_enabled,
                    copy_to_clipboard,
                    show_in_dock,
                    dictation_paused,
                    dictation_shortcuts: decode_shortcuts(&dictation_shortcuts),
                    onboarding_status: OnboardingStatus::from_storage_value(&onboarding_status)
                        .ok_or_else(|| {
                            StorageError::Invalid(format!(
                                "unsupported onboarding status: {onboarding_status}"
                            ))
                        })?,
                    onboarding_step: OnboardingStep::from_index(onboarding_step).ok_or_else(
                        || {
                            StorageError::Invalid(format!(
                                "unsupported onboarding step: {onboarding_step}"
                            ))
                        },
                    )?,
                })
            },
        )
}

pub(super) fn save(
    connection: &mut Connection,
    settings: &LocalSettings,
) -> Result<(), StorageError> {
    connection
        .execute(
            "UPDATE app_settings SET
                history_enabled = ?1,
                history_retention_days = ?2,
                preferred_microphone_id = ?3,
                preferred_microphone_name = ?4,
                diagnostics_logging_enabled = ?5,
                ui_language = ?6,
                automatic_update_checks = ?7,
                feedback_sounds_enabled = ?8,
                copy_to_clipboard = ?9,
                show_in_dock = ?10,
                dictation_paused = ?11,
                dictation_shortcuts = ?12,
                onboarding_status = ?13,
                onboarding_step = ?14
             WHERE singleton = 1",
            params![
                settings.history_enabled,
                settings.history_retention.days(),
                settings.preferred_microphone_id,
                settings.preferred_microphone_name,
                settings.diagnostics_logging_enabled,
                settings.ui_language.storage_value(),
                settings.automatic_update_checks,
                settings.feedback_sounds_enabled,
                settings.copy_to_clipboard,
                settings.show_in_dock,
                settings.dictation_paused,
                encode_shortcuts(&settings.dictation_shortcuts),
                settings.onboarding_status.storage_value(),
                settings.onboarding_step.index(),
            ],
        )
        .map_err(unavailable)?;
    Ok(())
}

fn encode_shortcuts(shortcuts: &[String]) -> String {
    shortcuts.join("\n")
}

fn decode_shortcuts(value: &str) -> Vec<String> {
    let shortcuts: Vec<_> = value
        .lines()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect();
    if shortcuts.is_empty() {
        vec!["right-command".to_owned()]
    } else {
        shortcuts
    }
}

fn retention_from_days(days: Option<u16>) -> Result<HistoryRetention, StorageError> {
    match days {
        Some(1) => Ok(HistoryRetention::OneDay),
        Some(7) => Ok(HistoryRetention::SevenDays),
        Some(30) => Ok(HistoryRetention::ThirtyDays),
        None => Ok(HistoryRetention::Forever),
        Some(other) => Err(StorageError::Invalid(format!(
            "unsupported history retention: {other} days"
        ))),
    }
}
