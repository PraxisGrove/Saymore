#![allow(clippy::panic_in_result_fn)]

use template_app::{ProviderCatalog, ProviderConfigStore};
use template_infra::{JsonSettingsStore, MOSS_MODEL_ID, MOSS_MODEL_REVISION, local_storage_usage};

#[test]
fn moss_selection_persists_without_enabling_cloud_asr() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("config.json");
    let store = JsonSettingsStore::at_path(path.clone());
    let mut catalog = ProviderCatalog::default();
    catalog.select_whisper_provider();
    catalog.select_moss_provider();
    store.save_catalog(&catalog)?;
    let reopened = JsonSettingsStore::at_path(path);
    let (settings, restored) = reopened.load_settings_snapshot()?;
    assert!(restored.moss_is_active());
    assert!(!restored.whisper_is_active());
    assert!(!settings.asr.volcengine.enabled);
    assert!(!settings.asr.openai_compatible.enabled);
    assert_eq!(catalog.asr_providers, restored.asr_providers);
    Ok(())
}

#[test]
fn moss_files_are_included_in_model_storage_usage() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let model = directory
        .path()
        .join("models")
        .join(MOSS_MODEL_ID)
        .join(MOSS_MODEL_REVISION);
    std::fs::create_dir_all(&model)?;
    std::fs::write(model.join("audio_encoder.onnx"), b"fixture")?;
    let usage = local_storage_usage(directory.path())?;
    assert_eq!(7, usage.models.moss_bytes);
    assert_eq!(7, usage.local_models_bytes);
    assert_eq!(7, usage.models.total_bytes());
    Ok(())
}
