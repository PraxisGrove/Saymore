use std::{error::Error, fmt, io, path::Path, time::SystemTime};

use template_app::{
    InstalledModel, InstalledModelStore, ProviderCatalog, ProviderConfigStore,
    SpeechRecognitionError,
};
use template_infra::VerifiedModelInstaller;

use super::LocalModel;
use crate::asr_runtime::AsrSessionController;

pub(super) trait ModelRuntime: Send + Sync {
    fn prepare(&self, model: LocalModel) -> Result<Option<u64>, SpeechRecognitionError>;
    fn clear(&self, model: LocalModel);
}

impl ModelRuntime for AsrSessionController {
    fn prepare(&self, model: LocalModel) -> Result<Option<u64>, SpeechRecognitionError> {
        match model {
            LocalModel::Paraformer => self.prepare_paraformer(),
            LocalModel::Whisper => self.prepare_whisper(),
            LocalModel::Qwen3 => self.prepare_qwen3(),
            LocalModel::SenseVoice => self.prepare_sense_voice(),
        }
    }

    fn clear(&self, model: LocalModel) {
        match model {
            LocalModel::Paraformer => self.clear_paraformer(),
            LocalModel::Whisper => self.clear_whisper(),
            LocalModel::Qwen3 => self.clear_qwen3(),
            LocalModel::SenseVoice => self.clear_sense_voice(),
        }
    }
}

pub(super) trait ModelFiles: Send + Sync {
    fn remove(&self) -> Result<(), String>;
}

impl ModelFiles for VerifiedModelInstaller {
    fn remove(&self) -> Result<(), String> {
        VerifiedModelInstaller::remove(self).map_err(|error| error.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum LifecycleError {
    ProviderChanged,
    ActiveModelCannotBeDeleted { model: &'static str },
    Unavailable(String),
}

impl fmt::Display for LifecycleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProviderChanged => {
                formatter.write_str("ASR provider changed while the model was loading")
            }
            Self::ActiveModelCannotBeDeleted { model } => {
                write!(formatter, "Switch providers before deleting {model}")
            }
            Self::Unavailable(message) => formatter.write_str(message),
        }
    }
}

impl Error for LifecycleError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ActivationOutcome {
    pub(super) memory_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ReconciledState {
    pub(super) selected: bool,
    pub(super) provider_selection_changed: bool,
}

pub(super) fn activate(
    settings: &dyn ProviderConfigStore,
    runtime: &dyn ModelRuntime,
    model: LocalModel,
) -> Result<ActivationOutcome, LifecycleError> {
    let catalog = load_catalog(settings)?;
    let expected_provider = catalog.active.asr.clone();
    let previous_model = active_local_model(&catalog);
    let memory_bytes = runtime
        .prepare(model)
        .map_err(|error| LifecycleError::Unavailable(error.to_string()))?;

    let mut current_catalog = match load_catalog(settings) {
        Ok(catalog) if catalog.active.asr == expected_provider => catalog,
        Ok(_) => {
            runtime.clear(model);
            return Err(LifecycleError::ProviderChanged);
        }
        Err(error) => {
            runtime.clear(model);
            return Err(error);
        }
    };
    model.select(&mut current_catalog);
    if let Err(error) = settings.save_catalog(&current_catalog) {
        runtime.clear(model);
        return Err(LifecycleError::Unavailable(error.to_string()));
    }
    if let Some(previous_model) = previous_model.filter(|previous| *previous != model) {
        runtime.clear(previous_model);
    }
    Ok(ActivationOutcome { memory_bytes })
}

pub(super) fn reconcile(
    settings: &dyn ProviderConfigStore,
    installed_models: &dyn InstalledModelStore,
    model: LocalModel,
    installed_path: Option<&Path>,
) -> Result<ReconciledState, LifecycleError> {
    let mut catalog = load_catalog(settings)?;
    let installed = installed_path.is_some();
    let selected = model.is_active(&catalog);
    let provider_selection_changed = selected && !installed && model.clear_selection(&mut catalog);
    if provider_selection_changed {
        settings
            .save_catalog(&catalog)
            .map_err(|error| LifecycleError::Unavailable(error.to_string()))?;
    }
    match installed_path {
        Some(path) => ensure_installation_recorded(installed_models, model, path)?,
        None => installed_models
            .delete_installed_model(model.id())
            .map_err(|error| LifecycleError::Unavailable(error.to_string()))?,
    }
    Ok(ReconciledState {
        selected: installed && selected,
        provider_selection_changed,
    })
}

pub(super) fn record_installation(
    installed_models: &dyn InstalledModelStore,
    model: LocalModel,
    path: &Path,
) -> Result<(), LifecycleError> {
    let installed_at_ms = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|error| LifecycleError::Unavailable(error.to_string()))?
        .as_millis()
        .try_into()
        .map_err(|error: std::num::TryFromIntError| {
            LifecycleError::Unavailable(error.to_string())
        })?;
    installed_models
        .save_installed_model(installed_model_record(model, path, installed_at_ms)?)
        .map_err(|error| LifecycleError::Unavailable(error.to_string()))
}

pub(super) fn delete(
    settings: &dyn ProviderConfigStore,
    installed_models: &dyn InstalledModelStore,
    runtime: &dyn ModelRuntime,
    files: &dyn ModelFiles,
    model: LocalModel,
) -> Result<(), LifecycleError> {
    let catalog = load_catalog(settings)?;
    if model.is_active(&catalog) {
        return Err(LifecycleError::ActiveModelCannotBeDeleted {
            model: model.installed_name(),
        });
    }
    runtime.clear(model);
    files.remove().map_err(LifecycleError::Unavailable)?;
    installed_models
        .delete_installed_model(model.id())
        .map_err(|error| LifecycleError::Unavailable(error.to_string()))
}

pub(super) fn prepare(
    runtime: &dyn ModelRuntime,
    model: LocalModel,
) -> Result<Option<u64>, SpeechRecognitionError> {
    runtime.prepare(model)
}

pub(super) fn clear(runtime: &dyn ModelRuntime, model: LocalModel) {
    runtime.clear(model);
}

pub(super) fn active_local_model(catalog: &ProviderCatalog) -> Option<LocalModel> {
    LocalModel::ALL
        .into_iter()
        .find(|model| model.is_active(catalog))
}

fn ensure_installation_recorded(
    installed_models: &dyn InstalledModelStore,
    model: LocalModel,
    path: &Path,
) -> Result<(), LifecycleError> {
    let already_recorded = installed_models
        .list_installed_models()
        .map_err(|error| LifecycleError::Unavailable(error.to_string()))?
        .iter()
        .any(|installed| installed.id == model.id());
    if already_recorded {
        Ok(())
    } else {
        record_installation(installed_models, model, path)
    }
}

fn installed_model_record(
    model: LocalModel,
    path: &Path,
    installed_at_ms: i64,
) -> Result<InstalledModel, LifecycleError> {
    let path = path.to_str().ok_or_else(|| {
        LifecycleError::Unavailable(
            io::Error::new(io::ErrorKind::InvalidData, "model path is not Unicode").to_string(),
        )
    })?;
    Ok(InstalledModel {
        id: model.id().to_owned(),
        provider_type: model.provider_type().to_owned(),
        model: model.installed_name().to_owned(),
        version: model.revision().to_owned(),
        path: path.to_owned(),
        installed_at_ms,
        last_verified_at_ms: Some(installed_at_ms),
    })
}

fn load_catalog(settings: &dyn ProviderConfigStore) -> Result<ProviderCatalog, LifecycleError> {
    settings
        .load_catalog()
        .map_err(|error| LifecycleError::Unavailable(error.to_string()))
}

#[cfg(test)]
mod tests;
