use std::sync::{Arc, Mutex};

use thiserror::Error;

use crate::{
    ColorSchemePreference, HistoryRetention, LocalSettings, LocalSettingsStore, OnboardingStatus,
    OnboardingStep, StorageError, ThemeId, UiLanguagePreference,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MicrophoneSelection {
    Automatic,
    Specific { id: String, name: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalSettingsChange {
    SetHistoryEnabled(bool),
    SetHistoryPolicy {
        enabled: bool,
        retention: HistoryRetention,
    },
    SelectMicrophone(MicrophoneSelection),
    SetUiLanguage(UiLanguagePreference),
    SetTheme(ThemeId),
    SetColorScheme(ColorSchemePreference),
    SetAutomaticUpdateChecks(bool),
    SetFeedbackSounds(bool),
    SetMuteSystemAudio(bool),
    SetCopyToClipboard(bool),
    SetDockVisibility(bool),
    SetDictationPaused(bool),
    SetDiagnosticsLogging(bool),
    ReplaceDictationShortcuts(Vec<String>),
    SetOnboardingProgress {
        status: OnboardingStatus,
        step: OnboardingStep,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LocalSettingsValidationError {
    #[error("the dictation shortcut collection must not be empty")]
    EmptyDictationShortcuts,
    #[error("the microphone identifier must not be blank")]
    BlankMicrophoneIdentifier,
    #[error("the microphone display name must not be blank")]
    BlankMicrophoneName,
    #[error("the microphone identifier and display name must be present together")]
    IncompleteMicrophoneSelection,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LocalSettingsMutationError {
    #[error("the local settings change is invalid: {0}")]
    InvalidChange(#[from] LocalSettingsValidationError),
    #[error("the local settings change could not be stored: {0}")]
    Storage(#[from] StorageError),
    #[error("local settings mutation is unavailable")]
    Unavailable,
}

/// Applies validated local product-preference changes without losing concurrent updates.
pub struct LocalSettingsMutator {
    store: Arc<dyn LocalSettingsStore>,
    coordinator: Mutex<()>,
}

impl LocalSettingsMutator {
    pub fn new(store: Arc<dyn LocalSettingsStore>) -> Self {
        Self {
            store,
            coordinator: Mutex::new(()),
        }
    }

    pub fn apply(
        &self,
        change: LocalSettingsChange,
    ) -> Result<LocalSettings, LocalSettingsMutationError> {
        validate_change(&change)?;
        let _coordinator = self
            .coordinator
            .lock()
            .map_err(|_| LocalSettingsMutationError::Unavailable)?;
        let mut settings = self.store.load_settings()?;
        apply_change(&mut settings, change);
        validate_settings(&settings)?;
        self.store.save_settings(settings.clone())?;
        Ok(settings)
    }
}

fn validate_settings(settings: &LocalSettings) -> Result<(), LocalSettingsValidationError> {
    match (
        settings.preferred_microphone_id.as_deref(),
        settings.preferred_microphone_name.as_deref(),
    ) {
        (Some(id), Some(_)) if id.trim().is_empty() => {
            Err(LocalSettingsValidationError::BlankMicrophoneIdentifier)
        }
        (Some(_), Some(name)) if name.trim().is_empty() => {
            Err(LocalSettingsValidationError::BlankMicrophoneName)
        }
        (Some(_), None) | (None, Some(_)) => {
            Err(LocalSettingsValidationError::IncompleteMicrophoneSelection)
        }
        _ if settings.dictation_shortcuts.is_empty() => {
            Err(LocalSettingsValidationError::EmptyDictationShortcuts)
        }
        _ => Ok(()),
    }
}

fn validate_change(change: &LocalSettingsChange) -> Result<(), LocalSettingsValidationError> {
    match change {
        LocalSettingsChange::SelectMicrophone(MicrophoneSelection::Specific { id, .. })
            if id.trim().is_empty() =>
        {
            Err(LocalSettingsValidationError::BlankMicrophoneIdentifier)
        }
        LocalSettingsChange::SelectMicrophone(MicrophoneSelection::Specific { name, .. })
            if name.trim().is_empty() =>
        {
            Err(LocalSettingsValidationError::BlankMicrophoneName)
        }
        LocalSettingsChange::ReplaceDictationShortcuts(shortcuts) if shortcuts.is_empty() => {
            Err(LocalSettingsValidationError::EmptyDictationShortcuts)
        }
        _ => Ok(()),
    }
}

fn apply_change(settings: &mut LocalSettings, change: LocalSettingsChange) {
    match change {
        LocalSettingsChange::SetHistoryEnabled(enabled) => settings.history_enabled = enabled,
        LocalSettingsChange::SetHistoryPolicy { enabled, retention } => {
            settings.history_enabled = enabled;
            settings.history_retention = retention;
        }
        LocalSettingsChange::SelectMicrophone(MicrophoneSelection::Automatic) => {
            settings.preferred_microphone_id = None;
            settings.preferred_microphone_name = None;
        }
        LocalSettingsChange::SelectMicrophone(MicrophoneSelection::Specific { id, name }) => {
            settings.preferred_microphone_id = Some(id);
            settings.preferred_microphone_name = Some(name);
        }
        LocalSettingsChange::SetUiLanguage(language) => settings.ui_language = language,
        LocalSettingsChange::SetTheme(theme) => settings.theme = theme,
        LocalSettingsChange::SetColorScheme(color_scheme) => {
            settings.color_scheme = color_scheme;
        }
        LocalSettingsChange::SetAutomaticUpdateChecks(enabled) => {
            settings.automatic_update_checks = enabled;
        }
        LocalSettingsChange::SetFeedbackSounds(enabled) => {
            settings.feedback_sounds_enabled = enabled;
        }
        LocalSettingsChange::SetMuteSystemAudio(enabled) => {
            settings.mute_system_audio_enabled = enabled;
        }
        LocalSettingsChange::SetCopyToClipboard(enabled) => settings.copy_to_clipboard = enabled,
        LocalSettingsChange::SetDockVisibility(visible) => settings.show_in_dock = visible,
        LocalSettingsChange::SetDictationPaused(paused) => settings.dictation_paused = paused,
        LocalSettingsChange::SetDiagnosticsLogging(enabled) => {
            settings.diagnostics_logging_enabled = enabled;
        }
        LocalSettingsChange::ReplaceDictationShortcuts(shortcuts) => {
            settings.dictation_shortcuts = shortcuts;
        }
        LocalSettingsChange::SetOnboardingProgress { status, step } => {
            settings.onboarding_status = status;
            settings.onboarding_step = step;
        }
    }
}
