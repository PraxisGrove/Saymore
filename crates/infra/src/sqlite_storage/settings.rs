use rusqlite::{Connection, OptionalExtension, Row, params};
use template_app::{
    ColorSchemePreference, HistoryRetention, LocalSettings, OnboardingStatus, OnboardingStep,
    StorageError, ThemeId, UiLanguagePreference,
};

use super::unavailable;

pub(super) fn load(connection: &Connection) -> Result<LocalSettings, StorageError> {
    let row = connection
        .query_row(
            "SELECT history_enabled, history_retention_days,
                    preferred_microphone_id, preferred_microphone_name,
                    diagnostics_logging_enabled, ui_language,
                    theme_id, color_scheme,
                    automatic_update_checks, feedback_sounds_enabled,
                    mute_system_audio_enabled,
                    copy_to_clipboard, show_in_dock, dictation_paused,
                    dictation_shortcuts, onboarding_status, onboarding_step
             FROM app_settings WHERE singleton = 1",
            [],
            StoredSettingsRow::read,
        )
        .optional()
        .map_err(unavailable)?
        .ok_or_else(|| StorageError::Invalid("app settings row is missing".to_owned()))?;
    row.into_settings()
}

struct StoredSettingsRow {
    history_enabled: bool,
    retention_days: Option<u16>,
    microphone_id: Option<String>,
    microphone_name: Option<String>,
    diagnostics_logging_enabled: bool,
    ui_language: String,
    theme_id: String,
    color_scheme: String,
    automatic_update_checks: bool,
    feedback_sounds_enabled: bool,
    mute_system_audio_enabled: bool,
    copy_to_clipboard: bool,
    show_in_dock: bool,
    dictation_paused: bool,
    dictation_shortcuts: String,
    onboarding_status: String,
    onboarding_step: u8,
}

impl StoredSettingsRow {
    fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            history_enabled: row.get(0)?,
            retention_days: row.get(1)?,
            microphone_id: row.get(2)?,
            microphone_name: row.get(3)?,
            diagnostics_logging_enabled: row.get(4)?,
            ui_language: row.get(5)?,
            theme_id: row.get(6)?,
            color_scheme: row.get(7)?,
            automatic_update_checks: row.get(8)?,
            feedback_sounds_enabled: row.get(9)?,
            mute_system_audio_enabled: row.get(10)?,
            copy_to_clipboard: row.get(11)?,
            show_in_dock: row.get(12)?,
            dictation_paused: row.get(13)?,
            dictation_shortcuts: row.get(14)?,
            onboarding_status: row.get(15)?,
            onboarding_step: row.get(16)?,
        })
    }

    fn into_settings(self) -> Result<LocalSettings, StorageError> {
        Ok(LocalSettings {
            history_enabled: self.history_enabled,
            history_retention: retention_from_days(self.retention_days)?,
            preferred_microphone_id: self.microphone_id,
            preferred_microphone_name: self.microphone_name,
            diagnostics_logging_enabled: self.diagnostics_logging_enabled,
            ui_language: UiLanguagePreference::from_storage_value(&self.ui_language).ok_or_else(
                || {
                    StorageError::Invalid(format!(
                        "unsupported UI language preference: {}",
                        self.ui_language
                    ))
                },
            )?,
            theme: ThemeId::from_storage_value(&self.theme_id).ok_or_else(|| {
                StorageError::Invalid(format!("unsupported theme identifier: {}", self.theme_id))
            })?,
            color_scheme: ColorSchemePreference::from_storage_value(&self.color_scheme)
                .ok_or_else(|| {
                    StorageError::Invalid(format!(
                        "unsupported color scheme preference: {}",
                        self.color_scheme
                    ))
                })?,
            automatic_update_checks: self.automatic_update_checks,
            feedback_sounds_enabled: self.feedback_sounds_enabled,
            mute_system_audio_enabled: self.mute_system_audio_enabled,
            copy_to_clipboard: self.copy_to_clipboard,
            show_in_dock: self.show_in_dock,
            dictation_paused: self.dictation_paused,
            dictation_shortcuts: decode_shortcuts(&self.dictation_shortcuts),
            onboarding_status: OnboardingStatus::from_storage_value(&self.onboarding_status)
                .ok_or_else(|| {
                    StorageError::Invalid(format!(
                        "unsupported onboarding status: {}",
                        self.onboarding_status
                    ))
                })?,
            onboarding_step: OnboardingStep::from_index(self.onboarding_step).ok_or_else(|| {
                StorageError::Invalid(format!(
                    "unsupported onboarding step: {}",
                    self.onboarding_step
                ))
            })?,
        })
    }
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
                theme_id = ?7,
                color_scheme = ?8,
                automatic_update_checks = ?9,
                feedback_sounds_enabled = ?10,
                mute_system_audio_enabled = ?11,
                copy_to_clipboard = ?12,
                show_in_dock = ?13,
                dictation_paused = ?14,
                dictation_shortcuts = ?15,
                onboarding_status = ?16,
                onboarding_step = ?17
             WHERE singleton = 1",
            params![
                settings.history_enabled,
                settings.history_retention.days(),
                settings.preferred_microphone_id,
                settings.preferred_microphone_name,
                settings.diagnostics_logging_enabled,
                settings.ui_language.storage_value(),
                settings.theme.storage_value(),
                settings.color_scheme.storage_value(),
                settings.automatic_update_checks,
                settings.feedback_sounds_enabled,
                settings.mute_system_audio_enabled,
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
    value
        .lines()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
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
