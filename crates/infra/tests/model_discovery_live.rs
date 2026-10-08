#![cfg(target_os = "macos")]

use std::{env, fs, path::PathBuf};

use template_app::LlmProviderPreset;
use template_infra::{ModelDiscoveryError, discover_models};

#[tokio::test]
#[ignore = "uses saved credentials for read-only GET /models acceptance"]
async fn fetches_directories_from_configured_providers() -> Result<(), Box<dyn std::error::Error>> {
    let home = env::var_os("HOME").ok_or("user home is unavailable")?;
    let path = PathBuf::from(home).join("Library/Application Support/Saymore/config.json");
    let bytes = fs::read(path).map_err(|_| "provider catalog is unavailable")?;
    let catalog: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| "provider catalog is not valid JSON")?;
    let providers = catalog
        .get("llm_providers")
        .and_then(serde_json::Value::as_array)
        .ok_or("provider catalog has no LLM directory")?;
    let mut tested = 0;
    let mut failures = Vec::new();
    for preset in [LlmProviderPreset::DeepSeek, LlmProviderPreset::Custom] {
        let Some(config) = providers
            .iter()
            .find(|provider| {
                provider.get("id").and_then(serde_json::Value::as_str) == Some(preset.id())
            })
            .and_then(|provider| provider.get("config"))
        else {
            continue;
        };
        let api_key = config
            .get("api_key")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if api_key.trim().is_empty() {
            continue;
        }
        let base_url = config
            .get("base_url")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let endpoint = preset.model_list_url_for(base_url);
        tested += 1;
        match discover_models(&endpoint, api_key).await {
            Ok(models) => println!(
                "{}: fetched {} normalized model IDs",
                preset.label(),
                models.len()
            ),
            Err(error) => failures.push(format!("{}: {}", preset.label(), error_kind(error))),
        }
    }
    if tested == 0 {
        return Err("no configured provider credentials are available".into());
    }
    if !failures.is_empty() {
        return Err(failures.join("; ").into());
    }
    Ok(())
}

fn error_kind(error: ModelDiscoveryError) -> &'static str {
    match error {
        ModelDiscoveryError::MissingApiKey => "missing API key",
        ModelDiscoveryError::Authentication => "authentication rejected",
        ModelDiscoveryError::RateLimited => "rate limited",
        ModelDiscoveryError::Empty => "empty directory",
        ModelDiscoveryError::Transport(_) => "transport failure",
        ModelDiscoveryError::Protocol(_) => "invalid model-list protocol",
    }
}
