#![allow(dead_code, unused_imports)]

#[path = "../../src/settings_ui/provider_mapping.rs"]
mod provider_mapping;
use provider_mapping::{VOLCENGINE_MODELS, provider_preset};

#[path = "../../src/settings_ui/model_discovery.rs"]
mod model_discovery;

pub fn wire(ui: &crate::ui::AppWindow, store: std::sync::Arc<template_infra::JsonSettingsStore>) {
    model_discovery::wire(ui, store);
}

pub fn restore_catalog(ui: &crate::ui::AppWindow, store: &template_infra::JsonSettingsStore) {
    use template_app::ProviderConfigStore;
    let catalog = match store.load_catalog() {
        Ok(catalog) => catalog,
        Err(error) => {
            eprintln!("cannot restore catalog: {error}");
            std::process::exit(1);
        }
    };
    model_discovery::restore_llm_model_catalog(
        ui,
        &catalog,
        template_app::LlmProviderPreset::Custom,
    );
}
