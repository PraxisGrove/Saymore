use std::collections::BTreeSet;

use serde_json::{Map, Value, json};

use super::{ChatCompletionsProfile, LlmProviderPreset, ProviderCatalog};

const MODEL_CATALOGS_KEY: &str = "model_catalogs";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmModelCatalog {
    pub models: Vec<String>,
    pub selected_model: String,
    pub refreshed_at_ms: i64,
}

impl ProviderCatalog {
    pub fn llm_model_catalog(
        &self,
        preset: LlmProviderPreset,
        model_list_url: &str,
        profile: ChatCompletionsProfile,
    ) -> Option<LlmModelCatalog> {
        let provider = &self.llm_providers[self.llm_provider_index(preset)?];
        let stored = provider.config.get(MODEL_CATALOGS_KEY)?.get(profile.id())?;
        decode_catalog(stored, model_list_url)
    }

    pub fn cache_llm_model_catalog(
        &mut self,
        preset: LlmProviderPreset,
        model_list_url: &str,
        profile: ChatCompletionsProfile,
        models: Vec<String>,
        selected_model: &str,
        refreshed_at_ms: i64,
    ) -> bool {
        let models = normalized_models(models);
        if models.is_empty() {
            return false;
        }
        if self.llm_provider_index(preset).is_none() {
            match preset {
                LlmProviderPreset::Custom => self.save_custom_llm_provider_config("", "", ""),
                preset => self.save_llm_provider_config(preset, ""),
            }
        }
        let Some(index) = self.llm_provider_index(preset) else {
            return false;
        };
        let selected_model = selected_model.trim();
        let selected_model = if models.iter().any(|model| model == selected_model) {
            selected_model
        } else {
            models.first().map(String::as_str).unwrap_or_default()
        };
        let Some(config) = self.llm_providers[index].config.as_object_mut() else {
            return false;
        };
        let catalogs = config
            .entry(MODEL_CATALOGS_KEY)
            .or_insert_with(|| Value::Object(Map::new()));
        let Some(catalogs) = catalogs.as_object_mut() else {
            return false;
        };
        catalogs.insert(
            profile.id().to_owned(),
            json!({
                "model_list_url": normalized_url(model_list_url),
                "models": models,
                "selected_model": selected_model,
                "refreshed_at_ms": refreshed_at_ms,
            }),
        );
        true
    }

    pub fn select_cached_llm_model(
        &mut self,
        preset: LlmProviderPreset,
        model_list_url: &str,
        profile: ChatCompletionsProfile,
        selected_model: &str,
    ) -> bool {
        let Some(index) = self.llm_provider_index(preset) else {
            return false;
        };
        let selected_model = selected_model.trim();
        let Some(catalog) = self.llm_providers[index]
            .config
            .get_mut(MODEL_CATALOGS_KEY)
            .and_then(Value::as_object_mut)
            .and_then(|catalogs| catalogs.get_mut(profile.id()))
        else {
            return false;
        };
        if catalog.get("model_list_url").and_then(Value::as_str)
            != Some(normalized_url(model_list_url).as_str())
            || !catalog
                .get("models")
                .and_then(Value::as_array)
                .is_some_and(|models| {
                    models
                        .iter()
                        .any(|model| model.as_str() == Some(selected_model))
                })
        {
            return false;
        }
        let Some(catalog) = catalog.as_object_mut() else {
            return false;
        };
        catalog.insert(
            "selected_model".to_owned(),
            Value::String(selected_model.to_owned()),
        );
        let Some(config) = self.llm_providers[index].config.as_object_mut() else {
            return false;
        };
        config.insert("model".to_owned(), Value::String(selected_model.to_owned()));
        true
    }
}

pub(super) fn preserve_model_catalogs(previous: &Value, next: &mut Value) {
    let Some(catalogs) = previous.get(MODEL_CATALOGS_KEY).cloned() else {
        return;
    };
    let Some(next) = next.as_object_mut() else {
        return;
    };
    next.insert(MODEL_CATALOGS_KEY.to_owned(), catalogs);
}

pub(super) fn update_cached_selection(
    config: &mut Value,
    model_list_url: &str,
    profile: ChatCompletionsProfile,
    selected_model: &str,
) {
    let Some(catalog) = config
        .get_mut(MODEL_CATALOGS_KEY)
        .and_then(Value::as_object_mut)
        .and_then(|catalogs| catalogs.get_mut(profile.id()))
    else {
        return;
    };
    if catalog.get("model_list_url").and_then(Value::as_str)
        != Some(normalized_url(model_list_url).as_str())
    {
        return;
    }
    let selected_model = selected_model.trim();
    if catalog
        .get("models")
        .and_then(Value::as_array)
        .is_some_and(|models| {
            models
                .iter()
                .any(|model| model.as_str() == Some(selected_model))
        })
        && let Some(catalog) = catalog.as_object_mut()
    {
        catalog.insert(
            "selected_model".to_owned(),
            Value::String(selected_model.to_owned()),
        );
    }
}

fn decode_catalog(value: &Value, model_list_url: &str) -> Option<LlmModelCatalog> {
    let stored_url = value.get("model_list_url")?.as_str()?;
    if normalized_url(stored_url) != normalized_url(model_list_url) {
        return None;
    }
    let models = value
        .get("models")?
        .as_array()?
        .iter()
        .map(Value::as_str)
        .collect::<Option<Vec<_>>>()?
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let selected_model = value.get("selected_model")?.as_str()?.to_owned();
    let refreshed_at_ms = value.get("refreshed_at_ms")?.as_i64()?;
    (!models.is_empty()).then_some(LlmModelCatalog {
        models,
        selected_model,
        refreshed_at_ms,
    })
}

fn normalized_models(models: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    models
        .into_iter()
        .map(|model| model.trim().to_owned())
        .filter(|model| !model.is_empty() && seen.insert(model.clone()))
        .collect()
}

fn normalized_url(url: &str) -> String {
    url.trim().trim_end_matches('/').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_catalogs_remain_separate_for_siliconflow_and_stepfun() {
        let mut catalog = ProviderCatalog::default();
        for (preset, models) in [
            (
                LlmProviderPreset::SiliconFlow,
                vec![
                    "Qwen/Qwen3-32B".to_owned(),
                    "deepseek-ai/DeepSeek-V3".to_owned(),
                ],
            ),
            (
                LlmProviderPreset::StepFun,
                vec!["step-3.5-flash".to_owned(), "step-3.5-mini".to_owned()],
            ),
        ] {
            assert!(catalog.cache_llm_model_catalog(
                preset,
                preset.model_list_url(),
                ChatCompletionsProfile::Portable,
                models,
                preset.model(),
                123,
            ));
        }

        let siliconflow = catalog
            .llm_model_catalog(
                LlmProviderPreset::SiliconFlow,
                LlmProviderPreset::SiliconFlow.model_list_url(),
                ChatCompletionsProfile::Portable,
            )
            .map(|catalog| catalog.models);
        let stepfun = catalog
            .llm_model_catalog(
                LlmProviderPreset::StepFun,
                LlmProviderPreset::StepFun.model_list_url(),
                ChatCompletionsProfile::Portable,
            )
            .map(|catalog| catalog.models);

        assert_eq!(
            Some(vec![
                "Qwen/Qwen3-32B".to_owned(),
                "deepseek-ai/DeepSeek-V3".to_owned()
            ]),
            siliconflow
        );
        assert_eq!(
            Some(vec![
                "step-3.5-flash".to_owned(),
                "step-3.5-mini".to_owned()
            ]),
            stepfun
        );
    }

    #[test]
    fn caches_models_by_provider_instance_endpoint_and_profile() {
        let mut catalog = ProviderCatalog::default();
        catalog.select_llm_provider(LlmProviderPreset::DeepSeek);

        assert!(catalog.cache_llm_model_catalog(
            LlmProviderPreset::DeepSeek,
            "https://api.deepseek.com/models",
            ChatCompletionsProfile::DeepSeek,
            vec!["deepseek-chat".to_owned(), "deepseek-reasoner".to_owned()],
            "deepseek-reasoner",
            123,
        ));

        let cached = catalog.llm_model_catalog(
            LlmProviderPreset::DeepSeek,
            "https://api.deepseek.com/models/",
            ChatCompletionsProfile::DeepSeek,
        );
        assert_eq!(
            Some(LlmModelCatalog {
                models: vec!["deepseek-chat".to_owned(), "deepseek-reasoner".to_owned()],
                selected_model: "deepseek-reasoner".to_owned(),
                refreshed_at_ms: 123,
            }),
            cached
        );
        assert_eq!(
            None,
            catalog.llm_model_catalog(
                LlmProviderPreset::DeepSeek,
                "https://proxy.example/models",
                ChatCompletionsProfile::DeepSeek,
            )
        );
        assert_eq!(
            None,
            catalog.llm_model_catalog(
                LlmProviderPreset::DeepSeek,
                "https://api.deepseek.com/models",
                ChatCompletionsProfile::Portable,
            )
        );
    }

    #[test]
    fn saving_provider_settings_preserves_catalog_and_updates_its_selection() {
        let mut catalog = ProviderCatalog::default();
        catalog.select_llm_provider(LlmProviderPreset::DeepSeek);
        assert!(catalog.cache_llm_model_catalog(
            LlmProviderPreset::DeepSeek,
            LlmProviderPreset::DeepSeek.model_list_url(),
            ChatCompletionsProfile::DeepSeek,
            vec!["deepseek-chat".to_owned(), "deepseek-reasoner".to_owned()],
            "deepseek-chat",
            456,
        ));

        catalog.save_llm_provider_model_config(
            LlmProviderPreset::DeepSeek,
            "saved-key",
            "deepseek-reasoner",
        );

        let cached = catalog.llm_model_catalog(
            LlmProviderPreset::DeepSeek,
            LlmProviderPreset::DeepSeek.model_list_url(),
            ChatCompletionsProfile::DeepSeek,
        );
        assert_eq!(
            Some("deepseek-reasoner"),
            cached
                .as_ref()
                .map(|catalog| catalog.selected_model.as_str())
        );
        assert_eq!(Some(456), cached.map(|catalog| catalog.refreshed_at_ms));
    }

    #[test]
    fn selecting_a_cached_model_updates_provider_and_catalog_selection() {
        let mut catalog = ProviderCatalog::default();
        catalog.save_llm_provider_model_config(LlmProviderPreset::Kimi, "saved-key", "kimi-k2.5");
        assert!(catalog.cache_llm_model_catalog(
            LlmProviderPreset::Kimi,
            LlmProviderPreset::Kimi.model_list_url(),
            ChatCompletionsProfile::Kimi,
            vec!["kimi-k2.5".to_owned(), "kimi-k2.6".to_owned()],
            "kimi-k2.5",
            456,
        ));

        assert!(catalog.select_cached_llm_model(
            LlmProviderPreset::Kimi,
            LlmProviderPreset::Kimi.model_list_url(),
            ChatCompletionsProfile::Kimi,
            "kimi-k2.6",
        ));
        assert_eq!(
            Some("kimi-k2.6"),
            catalog.configured_llm_provider_model(LlmProviderPreset::Kimi)
        );
        assert_eq!(
            Some("kimi-k2.6"),
            catalog
                .llm_model_catalog(
                    LlmProviderPreset::Kimi,
                    LlmProviderPreset::Kimi.model_list_url(),
                    ChatCompletionsProfile::Kimi,
                )
                .as_ref()
                .map(|catalog| catalog.selected_model.as_str())
        );
    }

    #[test]
    fn cache_normalizes_models_and_falls_back_to_the_first_available_selection() {
        let mut catalog = ProviderCatalog::default();
        catalog.select_llm_provider(LlmProviderPreset::DeepSeek);

        assert!(catalog.cache_llm_model_catalog(
            LlmProviderPreset::DeepSeek,
            LlmProviderPreset::DeepSeek.model_list_url(),
            ChatCompletionsProfile::DeepSeek,
            vec![
                " deepseek-chat ".to_owned(),
                String::new(),
                "deepseek-chat".to_owned(),
                "deepseek-reasoner".to_owned(),
            ],
            "missing-model",
            789,
        ));

        let cached = catalog.llm_model_catalog(
            LlmProviderPreset::DeepSeek,
            LlmProviderPreset::DeepSeek.model_list_url(),
            ChatCompletionsProfile::DeepSeek,
        );
        assert_eq!(
            Some(vec![
                "deepseek-chat".to_owned(),
                "deepseek-reasoner".to_owned()
            ]),
            cached.as_ref().map(|catalog| catalog.models.clone())
        );
        assert_eq!(
            Some("deepseek-chat"),
            cached
                .as_ref()
                .map(|catalog| catalog.selected_model.as_str())
        );
    }

    #[test]
    fn first_refresh_creates_an_inactive_provider_configuration_instance() {
        let mut catalog = ProviderCatalog::default();

        assert!(catalog.cache_llm_model_catalog(
            LlmProviderPreset::DeepSeek,
            LlmProviderPreset::DeepSeek.model_list_url(),
            ChatCompletionsProfile::DeepSeek,
            vec!["deepseek-chat".to_owned()],
            "deepseek-chat",
            987,
        ));

        assert_eq!(None, catalog.active.llm);
        assert_eq!(1, catalog.llm_providers.len());
        assert!(
            catalog
                .llm_model_catalog(
                    LlmProviderPreset::DeepSeek,
                    LlmProviderPreset::DeepSeek.model_list_url(),
                    ChatCompletionsProfile::DeepSeek,
                )
                .is_some()
        );
    }

    #[test]
    fn unsuccessful_cache_update_keeps_the_previous_catalog() {
        let mut catalog = ProviderCatalog::default();
        assert!(catalog.cache_llm_model_catalog(
            LlmProviderPreset::DeepSeek,
            LlmProviderPreset::DeepSeek.model_list_url(),
            ChatCompletionsProfile::DeepSeek,
            vec!["deepseek-chat".to_owned(), "deepseek-reasoner".to_owned()],
            "deepseek-chat",
            1_000,
        ));

        assert!(!catalog.cache_llm_model_catalog(
            LlmProviderPreset::DeepSeek,
            LlmProviderPreset::DeepSeek.model_list_url(),
            ChatCompletionsProfile::DeepSeek,
            Vec::new(),
            "",
            2_000,
        ));

        let cached = catalog.llm_model_catalog(
            LlmProviderPreset::DeepSeek,
            LlmProviderPreset::DeepSeek.model_list_url(),
            ChatCompletionsProfile::DeepSeek,
        );
        assert_eq!(
            Some(vec![
                "deepseek-chat".to_owned(),
                "deepseek-reasoner".to_owned()
            ]),
            cached.as_ref().map(|catalog| catalog.models.clone())
        );
        assert_eq!(Some(1_000), cached.map(|catalog| catalog.refreshed_at_ms));
    }
}
