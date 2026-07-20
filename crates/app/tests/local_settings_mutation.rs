use std::{
    sync::{
        Arc, Barrier, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
};

use template_app::{
    ColorSchemePreference, HistoryRetention, LocalSettings, LocalSettingsChange,
    LocalSettingsMutationError, LocalSettingsMutator, LocalSettingsStore,
    LocalSettingsValidationError, MicrophoneSelection, OnboardingStatus, OnboardingStep,
    StorageError, ThemeId, UiLanguagePreference,
};

struct FakeSettingsStore {
    settings: Mutex<LocalSettings>,
    loads: AtomicUsize,
    saves: AtomicUsize,
    fail_saves: AtomicBool,
}

impl FakeSettingsStore {
    fn new(settings: LocalSettings) -> Self {
        Self {
            settings: Mutex::new(settings),
            loads: AtomicUsize::new(0),
            saves: AtomicUsize::new(0),
            fail_saves: AtomicBool::new(false),
        }
    }

    fn snapshot(&self) -> Result<LocalSettings, StorageError> {
        self.settings
            .lock()
            .map(|settings| settings.clone())
            .map_err(|_| StorageError::Unavailable("fake settings lock was poisoned".to_owned()))
    }
}

impl LocalSettingsStore for FakeSettingsStore {
    fn load_settings(&self) -> Result<LocalSettings, StorageError> {
        self.loads.fetch_add(1, Ordering::Relaxed);
        self.snapshot()
    }

    fn save_settings(&self, settings: LocalSettings) -> Result<(), StorageError> {
        self.saves.fetch_add(1, Ordering::Relaxed);
        if self.fail_saves.load(Ordering::Relaxed) {
            return Err(StorageError::Unavailable(
                "injected save failure".to_owned(),
            ));
        }
        self.settings
            .lock()
            .map(|mut saved| *saved = settings)
            .map_err(|_| StorageError::Unavailable("fake settings lock was poisoned".to_owned()))
    }
}

fn changed(initial: &LocalSettings, update: impl FnOnce(&mut LocalSettings)) -> LocalSettings {
    let mut expected = initial.clone();
    update(&mut expected);
    expected
}

#[test]
fn changing_history_enabled_preserves_unrelated_settings() {
    let initial = LocalSettings {
        feedback_sounds_enabled: false,
        copy_to_clipboard: true,
        ..LocalSettings::default()
    };
    let store = Arc::new(FakeSettingsStore::new(initial.clone()));
    let mutator = LocalSettingsMutator::new(store);

    let committed = mutator.apply(LocalSettingsChange::SetHistoryEnabled(false));

    assert_eq!(
        Ok(LocalSettings {
            history_enabled: false,
            ..initial
        }),
        committed
    );
}

#[test]
fn every_change_commits_the_expected_complete_snapshot() {
    let initial = LocalSettings::default();
    let cases = [
        (
            LocalSettingsChange::SetHistoryPolicy {
                enabled: false,
                retention: HistoryRetention::ThirtyDays,
            },
            changed(&initial, |settings| {
                settings.history_enabled = false;
                settings.history_retention = HistoryRetention::ThirtyDays;
            }),
        ),
        (
            LocalSettingsChange::SelectMicrophone(MicrophoneSelection::Specific {
                id: "mic-id".to_owned(),
                name: "Desk microphone".to_owned(),
            }),
            changed(&initial, |settings| {
                settings.preferred_microphone_id = Some("mic-id".to_owned());
                settings.preferred_microphone_name = Some("Desk microphone".to_owned());
            }),
        ),
        (
            LocalSettingsChange::SetUiLanguage(UiLanguagePreference::SimplifiedChinese),
            changed(&initial, |settings| {
                settings.ui_language = UiLanguagePreference::SimplifiedChinese;
            }),
        ),
        (
            LocalSettingsChange::SetTheme(ThemeId::IrisMist),
            changed(&initial, |settings| settings.theme = ThemeId::IrisMist),
        ),
        (
            LocalSettingsChange::SetColorScheme(ColorSchemePreference::Dark),
            changed(&initial, |settings| {
                settings.color_scheme = ColorSchemePreference::Dark;
            }),
        ),
        (
            LocalSettingsChange::SetAutomaticUpdateChecks(true),
            changed(&initial, |settings| settings.automatic_update_checks = true),
        ),
        (
            LocalSettingsChange::SetFeedbackSounds(false),
            changed(&initial, |settings| {
                settings.feedback_sounds_enabled = false
            }),
        ),
        (
            LocalSettingsChange::SetMuteSystemAudio(false),
            changed(&initial, |settings| {
                settings.mute_system_audio_enabled = false
            }),
        ),
        (
            LocalSettingsChange::SetCopyToClipboard(true),
            changed(&initial, |settings| settings.copy_to_clipboard = true),
        ),
        (
            LocalSettingsChange::SetDockVisibility(false),
            changed(&initial, |settings| settings.show_in_dock = false),
        ),
        (
            LocalSettingsChange::SetDictationPaused(true),
            changed(&initial, |settings| settings.dictation_paused = true),
        ),
        (
            LocalSettingsChange::SetDiagnosticsLogging(true),
            changed(&initial, |settings| {
                settings.diagnostics_logging_enabled = true
            }),
        ),
        (
            LocalSettingsChange::ReplaceDictationShortcuts(vec!["left-command".to_owned()]),
            changed(&initial, |settings| {
                settings.dictation_shortcuts = vec!["left-command".to_owned()];
            }),
        ),
        (
            LocalSettingsChange::SetOnboardingProgress {
                status: OnboardingStatus::Completed,
                step: OnboardingStep::Complete,
            },
            changed(&initial, |settings| {
                settings.onboarding_status = OnboardingStatus::Completed;
                settings.onboarding_step = OnboardingStep::Complete;
            }),
        ),
    ];

    for (change, expected) in cases {
        let mutator = LocalSettingsMutator::new(Arc::new(FakeSettingsStore::new(initial.clone())));
        assert_eq!(Ok(expected), mutator.apply(change));
    }
}

#[test]
fn automatic_microphone_selection_clears_both_stored_fields() {
    let initial = LocalSettings {
        preferred_microphone_id: Some("mic-id".to_owned()),
        preferred_microphone_name: Some("Desk microphone".to_owned()),
        ..LocalSettings::default()
    };
    let mutator = LocalSettingsMutator::new(Arc::new(FakeSettingsStore::new(initial.clone())));

    assert_eq!(
        Ok(changed(&initial, |settings| {
            settings.preferred_microphone_id = None;
            settings.preferred_microphone_name = None;
        })),
        mutator.apply(LocalSettingsChange::SelectMicrophone(
            MicrophoneSelection::Automatic,
        ))
    );
}

#[test]
fn removing_every_shortcut_is_persisted() {
    let store = Arc::new(FakeSettingsStore::new(LocalSettings::default()));
    let mutator = LocalSettingsMutator::new(store.clone());

    let result = mutator.apply(LocalSettingsChange::ReplaceDictationShortcuts(Vec::new()));

    assert_eq!(
        Some(Vec::new()),
        result.ok().map(|settings| settings.dictation_shortcuts)
    );
    assert_eq!(1, store.loads.load(Ordering::Relaxed));
    assert_eq!(1, store.saves.load(Ordering::Relaxed));
}

#[test]
fn blank_microphone_fields_are_rejected() {
    for (id, name, expected) in [
        (
            " ",
            "Desk microphone",
            LocalSettingsValidationError::BlankMicrophoneIdentifier,
        ),
        (
            "mic-id",
            " ",
            LocalSettingsValidationError::BlankMicrophoneName,
        ),
    ] {
        let store = Arc::new(FakeSettingsStore::new(LocalSettings::default()));
        let mutator = LocalSettingsMutator::new(store.clone());
        assert_eq!(
            Err(LocalSettingsMutationError::InvalidChange(expected)),
            mutator.apply(LocalSettingsChange::SelectMicrophone(
                MicrophoneSelection::Specific {
                    id: id.to_owned(),
                    name: name.to_owned(),
                },
            ))
        );
        assert_eq!(0, store.loads.load(Ordering::Relaxed));
        assert_eq!(0, store.saves.load(Ordering::Relaxed));
    }
}

#[test]
fn invalid_loaded_combination_is_not_saved_by_an_unrelated_change() {
    let store = Arc::new(FakeSettingsStore::new(LocalSettings {
        preferred_microphone_id: Some("mic-id".to_owned()),
        preferred_microphone_name: None,
        ..LocalSettings::default()
    }));
    let mutator = LocalSettingsMutator::new(store.clone());

    assert_eq!(
        Err(LocalSettingsMutationError::InvalidChange(
            LocalSettingsValidationError::IncompleteMicrophoneSelection
        )),
        mutator.apply(LocalSettingsChange::SetCopyToClipboard(true))
    );
    assert_eq!(0, store.saves.load(Ordering::Relaxed));
}

#[test]
fn save_failure_does_not_return_or_store_the_candidate_snapshot() {
    let initial = LocalSettings::default();
    let store = Arc::new(FakeSettingsStore::new(initial.clone()));
    store.fail_saves.store(true, Ordering::Relaxed);
    let mutator = LocalSettingsMutator::new(store.clone());

    let result = mutator.apply(LocalSettingsChange::SetCopyToClipboard(true));

    assert!(matches!(
        result,
        Err(LocalSettingsMutationError::Storage(
            StorageError::Unavailable(_)
        ))
    ));
    assert_eq!(Ok(initial), store.snapshot());
}

#[test]
fn concurrent_changes_preserve_each_others_fields() {
    let store = Arc::new(FakeSettingsStore::new(LocalSettings::default()));
    let mutator = Arc::new(LocalSettingsMutator::new(store.clone()));
    let start = Arc::new(Barrier::new(3));
    let feedback = spawn_change(
        Arc::clone(&mutator),
        Arc::clone(&start),
        LocalSettingsChange::SetFeedbackSounds(false),
    );
    let clipboard = spawn_change(
        Arc::clone(&mutator),
        Arc::clone(&start),
        LocalSettingsChange::SetCopyToClipboard(true),
    );
    start.wait();

    assert!(feedback.join().is_ok());
    assert!(clipboard.join().is_ok());
    assert_eq!(
        Ok(LocalSettings {
            feedback_sounds_enabled: false,
            copy_to_clipboard: true,
            ..LocalSettings::default()
        }),
        store.snapshot()
    );
}

fn spawn_change(
    mutator: Arc<LocalSettingsMutator>,
    start: Arc<Barrier>,
    change: LocalSettingsChange,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        start.wait();
        assert!(mutator.apply(change).is_ok());
    })
}
