use std::sync::{Arc, Mutex};

use template_app::{
    InstalledModel, InstalledModelStore, ProviderCatalog, ProviderConfigStore, SettingsStoreError,
    SpeechRecognitionError, StorageError,
};

use super::*;

#[test]
fn activation_switches_only_after_prepare_succeeds() {
    let settings = FakeSettings::with_active(LocalModel::Whisper);
    let runtime = FakeRuntime::default();

    let result = activate(&settings, &runtime, LocalModel::Qwen3);

    assert_eq!(
        Ok(ActivationOutcome {
            memory_bytes: Some(42)
        }),
        result
    );
    assert!(settings.catalog().qwen3_asr_is_active());
    assert_eq!(vec!["prepare:qwen3", "clear:whisper"], runtime.events());
}

#[test]
fn failed_prepare_keeps_the_previous_provider_loaded_and_selected() {
    let settings = FakeSettings::with_active(LocalModel::Whisper);
    let runtime = FakeRuntime::failing_prepare();

    let result = activate(&settings, &runtime, LocalModel::Qwen3);

    assert!(matches!(result, Err(LifecycleError::Unavailable(_))));
    assert!(settings.catalog().whisper_is_active());
    assert_eq!(vec!["prepare:qwen3"], runtime.events());
}

#[test]
fn provider_change_during_loading_discards_the_new_runtime() {
    let settings = Arc::new(FakeSettings::with_active(LocalModel::Whisper));
    let runtime = FakeRuntime::changing_provider(Arc::clone(&settings));

    let result = activate(settings.as_ref(), &runtime, LocalModel::Qwen3);

    assert_eq!(Err(LifecycleError::ProviderChanged), result);
    assert!(settings.catalog().paraformer_is_active());
    assert_eq!(vec!["prepare:qwen3", "clear:qwen3"], runtime.events());
}

#[test]
fn provider_save_failure_discards_the_new_runtime_and_keeps_the_previous_provider() {
    let settings = FakeSettings::failing_save(LocalModel::Whisper);
    let runtime = FakeRuntime::default();

    let result = activate(&settings, &runtime, LocalModel::Qwen3);

    assert!(matches!(result, Err(LifecycleError::Unavailable(_))));
    assert!(settings.catalog().whisper_is_active());
    assert_eq!(vec!["prepare:qwen3", "clear:qwen3"], runtime.events());
}

#[test]
fn startup_reconciliation_clears_a_missing_selected_model_and_metadata() {
    let settings = FakeSettings::with_active(LocalModel::Qwen3);
    let installed = FakeInstalledModels::with_model(LocalModel::Qwen3);

    let result = reconcile(&settings, &installed, LocalModel::Qwen3, None);

    assert_eq!(
        Ok(ReconciledState {
            selected: false,
            provider_selection_changed: true,
        }),
        result
    );
    assert!(settings.catalog().active.asr.is_none());
    assert!(installed.models().is_empty());
}

#[test]
fn startup_reconciliation_records_an_installed_model_once() {
    let settings = FakeSettings::default();
    let installed = FakeInstalledModels::default();
    let path = std::path::Path::new("/models/qwen3");

    assert!(reconcile(&settings, &installed, LocalModel::Qwen3, Some(path)).is_ok());
    assert!(reconcile(&settings, &installed, LocalModel::Qwen3, Some(path)).is_ok());

    let models = installed.models();
    assert_eq!(1, models.len());
    assert_eq!(LocalModel::Qwen3.id(), models[0].id);
    assert_eq!(path.to_str(), Some(models[0].path.as_str()));
}

#[test]
fn deletion_rejects_the_current_model_before_touching_files() {
    let settings = FakeSettings::with_active(LocalModel::SenseVoice);
    let installed = FakeInstalledModels::with_model(LocalModel::SenseVoice);
    let runtime = FakeRuntime::default();
    let files = FakeFiles::default();

    let result = delete(
        &settings,
        &installed,
        &runtime,
        &files,
        LocalModel::SenseVoice,
    );

    assert!(matches!(
        result,
        Err(LifecycleError::ActiveModelCannotBeDeleted { .. })
    ));
    assert!(runtime.events().is_empty());
    assert!(!files.removed());
    assert_eq!(1, installed.models().len());
}

#[test]
fn deletion_releases_runtime_removes_files_and_deletes_metadata() {
    let settings = FakeSettings::default();
    let installed = FakeInstalledModels::with_model(LocalModel::SenseVoice);
    let runtime = FakeRuntime::default();
    let files = FakeFiles::default();

    let result = delete(
        &settings,
        &installed,
        &runtime,
        &files,
        LocalModel::SenseVoice,
    );

    assert_eq!(Ok(()), result);
    assert_eq!(vec!["clear:sense-voice"], runtime.events());
    assert!(files.removed());
    assert!(installed.models().is_empty());
}

#[derive(Default)]
struct FakeSettings {
    catalog: Mutex<ProviderCatalog>,
    fail_save: bool,
}

impl FakeSettings {
    fn with_active(model: LocalModel) -> Self {
        let mut catalog = ProviderCatalog::default();
        model.select(&mut catalog);
        Self {
            catalog: Mutex::new(catalog),
            fail_save: false,
        }
    }

    fn failing_save(model: LocalModel) -> Self {
        let mut settings = Self::with_active(model);
        settings.fail_save = true;
        settings
    }

    fn catalog(&self) -> ProviderCatalog {
        self.catalog
            .lock()
            .map(|value| value.clone())
            .unwrap_or_default()
    }
}

impl ProviderConfigStore for FakeSettings {
    fn load_catalog(&self) -> Result<ProviderCatalog, SettingsStoreError> {
        self.catalog
            .lock()
            .map(|value| value.clone())
            .map_err(|_| SettingsStoreError::Unavailable("fake lock poisoned".to_owned()))
    }

    fn save_catalog(&self, catalog: &ProviderCatalog) -> Result<(), SettingsStoreError> {
        if self.fail_save {
            return Err(SettingsStoreError::Unavailable(
                "fake save failed".to_owned(),
            ));
        }
        self.catalog
            .lock()
            .map(|mut value| *value = catalog.clone())
            .map_err(|_| SettingsStoreError::Unavailable("fake lock poisoned".to_owned()))
    }
}

#[derive(Default)]
struct FakeRuntime {
    events: Mutex<Vec<String>>,
    fail_prepare: bool,
    change_provider: Option<Arc<FakeSettings>>,
}

impl FakeRuntime {
    fn failing_prepare() -> Self {
        Self {
            fail_prepare: true,
            ..Self::default()
        }
    }

    fn changing_provider(settings: Arc<FakeSettings>) -> Self {
        Self {
            change_provider: Some(settings),
            ..Self::default()
        }
    }

    fn events(&self) -> Vec<String> {
        self.events
            .lock()
            .map(|events| events.clone())
            .unwrap_or_default()
    }
}

impl ModelRuntime for FakeRuntime {
    fn prepare(&self, model: LocalModel) -> Result<Option<u64>, SpeechRecognitionError> {
        if let Ok(mut events) = self.events.lock() {
            events.push(format!("prepare:{}", short_name(model)));
        }
        if let Some(settings) = &self.change_provider
            && let Ok(mut catalog) = settings.catalog.lock()
        {
            LocalModel::Paraformer.select(&mut catalog);
        }
        if self.fail_prepare {
            Err(SpeechRecognitionError::Protocol("load failed".to_owned()))
        } else {
            Ok(Some(42))
        }
    }

    fn clear(&self, model: LocalModel) {
        if let Ok(mut events) = self.events.lock() {
            events.push(format!("clear:{}", short_name(model)));
        }
    }
}

#[derive(Default)]
struct FakeInstalledModels {
    models: Mutex<Vec<InstalledModel>>,
}

impl FakeInstalledModels {
    fn with_model(model: LocalModel) -> Self {
        let record = installed_model_record(model, std::path::Path::new("/models/model"), 7)
            .unwrap_or_else(|error| panic!("record should be valid: {error}"));
        Self {
            models: Mutex::new(vec![record]),
        }
    }

    fn models(&self) -> Vec<InstalledModel> {
        self.models
            .lock()
            .map(|models| models.clone())
            .unwrap_or_default()
    }
}

impl InstalledModelStore for FakeInstalledModels {
    fn list_installed_models(&self) -> Result<Vec<InstalledModel>, StorageError> {
        self.models
            .lock()
            .map(|models| models.clone())
            .map_err(|_| StorageError::Unavailable("fake lock poisoned".to_owned()))
    }

    fn save_installed_model(&self, model: InstalledModel) -> Result<(), StorageError> {
        self.models
            .lock()
            .map(|mut models| {
                models.retain(|existing| existing.id != model.id);
                models.push(model);
            })
            .map_err(|_| StorageError::Unavailable("fake lock poisoned".to_owned()))
    }

    fn delete_installed_model(&self, id: &str) -> Result<(), StorageError> {
        self.models
            .lock()
            .map(|mut models| models.retain(|model| model.id != id))
            .map_err(|_| StorageError::Unavailable("fake lock poisoned".to_owned()))
    }
}

#[derive(Default)]
struct FakeFiles {
    removed: Mutex<bool>,
}

impl FakeFiles {
    fn removed(&self) -> bool {
        self.removed.lock().is_ok_and(|removed| *removed)
    }
}

impl ModelFiles for FakeFiles {
    fn remove(&self) -> Result<(), String> {
        self.removed
            .lock()
            .map(|mut removed| *removed = true)
            .map_err(|_| "fake lock poisoned".to_owned())
    }
}

fn short_name(model: LocalModel) -> &'static str {
    match model {
        LocalModel::Paraformer => "paraformer",
        LocalModel::Whisper => "whisper",
        LocalModel::Qwen3 => "qwen3",
        LocalModel::SenseVoice => "sense-voice",
    }
}
