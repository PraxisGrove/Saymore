use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use template_app::{
    ChatCompletionsLlmSettings, DictionaryEntry, FinalTextProcessor, FinalTextRequest,
    ProviderConfigStore, ProviderInstance, RefinementEvaluationMode, RefinementFallbackReason,
    RefinementMode, RefinementSkipReason, RefinementStatus, SpeechRecognitionHints,
    StreamingSpeechRecognizer, relevant_dictionary_terms_from_entries,
};
use template_infra::{
    AppEnvironment, AppPaths, ChatCompletionsLlmProvider, JsonSettingsStore,
    VolcengineSpeechRecognizer, read_dictionary_snapshot,
};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use super::{
    local_correction::{LocalCorrectionResult, correct},
    metrics::text_metrics,
    rules::{EvaluationRule, RuleResult, evaluate_rules, pass_rate},
    wav::pcm16_mono_16khz,
};

const AUDIO_CHUNK_SAMPLES: usize = 1_600;

#[derive(Debug, Deserialize)]
struct EvaluationRequest {
    run_id: String,
    environment: String,
    case_ids: Vec<String>,
    asr_provider_id: String,
    llm_provider_ids: Vec<String>,
    hotwords_enabled: bool,
    #[serde(default)]
    force_refinement: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct EvaluationCase {
    id: String,
    category: String,
    title: String,
    read: String,
    expected: String,
    #[serde(default)]
    rules: Vec<EvaluationRule>,
}

#[derive(Debug, Serialize)]
struct ProviderList {
    environment: &'static str,
    asr: Vec<ProviderDescriptor>,
    llm: Vec<ProviderDescriptor>,
}

#[derive(Debug, Clone, Serialize)]
struct ProviderDescriptor {
    id: String,
    name: String,
    provider_type: String,
    model: String,
    configured: bool,
    consented: bool,
}

#[derive(Debug, Serialize)]
struct EvaluationResult {
    run_id: String,
    environment: String,
    started_at: String,
    completed_at: String,
    prompt_version: &'static str,
    hotwords_enabled: bool,
    force_refinement: bool,
    dictionary_terms: usize,
    cases: Vec<CaseResult>,
}

#[derive(Debug, Serialize)]
struct CaseResult {
    case_id: String,
    category: String,
    title: String,
    asr_reference: String,
    expected: String,
    asr: AsrResult,
    local: Option<LocalCorrectionResult>,
    llm: Vec<LlmResult>,
}

#[derive(Debug, Serialize)]
struct AsrResult {
    provider_id: String,
    provider_name: String,
    model: String,
    transcript: Option<String>,
    error: Option<String>,
    duration_ms: u128,
    surface_character_error_rate: Option<f64>,
    content_character_error_rate: Option<f64>,
    punctuation_score: Option<f64>,
}

#[derive(Debug, Serialize)]
struct LlmResult {
    provider_id: String,
    provider_name: String,
    model: String,
    text: Option<String>,
    provider_output: Option<String>,
    status: String,
    error: Option<String>,
    duration_ms: u128,
    exact_match: bool,
    surface_character_error_rate: Option<f64>,
    content_character_error_rate: Option<f64>,
    punctuation_score: Option<f64>,
    structure_match: Option<bool>,
    rule_results: Vec<RuleResult>,
    rule_pass_rate: Option<f64>,
}

#[derive(Debug, Serialize)]
struct Progress<'a> {
    completed: usize,
    total: usize,
    current_case: Option<&'a str>,
    phase: Option<&'a str>,
}

struct SelectedAsr {
    descriptor: ProviderDescriptor,
    api_key: String,
}

#[derive(Clone)]
struct SelectedLlm {
    descriptor: ProviderDescriptor,
    processor: Arc<FinalTextProcessor>,
}

pub fn print_providers(environment: AppEnvironment) -> Result<()> {
    let catalog = load_catalog(environment)?;
    let response = ProviderList {
        environment: environment_name(environment),
        asr: catalog
            .asr_providers
            .iter()
            .map(provider_descriptor)
            .collect(),
        llm: catalog
            .llm_providers
            .iter()
            .map(provider_descriptor)
            .collect(),
    };
    println!("{}", serde_json::to_string(&response)?);
    Ok(())
}

#[tokio::main]
pub async fn run_evaluation(
    request_path: &Path,
    manifest_path: &Path,
    recordings_root: &Path,
    output_path: &Path,
) -> Result<()> {
    let request: EvaluationRequest = read_json(request_path)?;
    let environment = parse_environment(&request.environment)?;
    validate_run_id(&request.run_id)?;
    if request.case_ids.is_empty() {
        bail!("at least one recorded case is required");
    }
    if request.llm_provider_ids.is_empty() {
        bail!("at least one LLM provider is required");
    }
    let manifest = read_json::<Vec<EvaluationCase>>(manifest_path)?;
    let cases = selected_cases(&manifest, &request.case_ids)?;
    let catalog = load_catalog(environment)?;
    let asr = selected_asr(&catalog.asr_providers, &request.asr_provider_id)?;
    let llms = selected_llms(&catalog.llm_providers, &request.llm_provider_ids)?;
    let dictionary = dictionary_snapshot(environment);
    let hints = if request.hotwords_enabled {
        SpeechRecognitionHints::from_terms(
            dictionary
                .iter()
                .map(|entry| entry.canonical.clone())
                .collect(),
        )
    } else {
        SpeechRecognitionHints::default()
    };
    let started_at = Utc::now().to_rfc3339();
    write_progress(output_path, 0, cases.len(), None, None)?;
    let mut results = Vec::with_capacity(cases.len());
    for (index, case) in cases.iter().enumerate() {
        write_progress(output_path, index, cases.len(), Some(&case.id), Some("asr"))?;
        let audio_path = recordings_root.join(&case.id).join("recording.wav");
        let samples = pcm16_mono_16khz(
            &fs::read(&audio_path)
                .with_context(|| format!("failed to read {}", audio_path.display()))?,
        )?;
        let asr_result = transcribe(&asr, hints.clone(), samples, &case.read).await;
        let (local_result, llm_results) = match asr_result.transcript.as_deref() {
            Some(transcript) => {
                let language = inferred_language(transcript);
                let relevant_terms = relevant_dictionary_terms_from_entries(
                    dictionary.clone(),
                    transcript,
                    language,
                )
                .unwrap_or_default();
                let local = correct(transcript, &case.expected, &relevant_terms, &case.rules);
                write_progress(output_path, index, cases.len(), Some(&case.id), Some("llm"))?;
                let llm = refine_all(
                    &llms,
                    transcript,
                    case,
                    relevant_terms,
                    request.force_refinement,
                )
                .await;
                (Some(local), llm)
            }
            None => (None, Vec::new()),
        };
        results.push(CaseResult {
            case_id: case.id.clone(),
            category: case.category.clone(),
            title: case.title.clone(),
            asr_reference: case.read.clone(),
            expected: case.expected.clone(),
            asr: asr_result,
            local: local_result,
            llm: llm_results,
        });
    }
    write_progress(output_path, cases.len(), cases.len(), None, None)?;
    let result = EvaluationResult {
        run_id: request.run_id,
        environment: request.environment,
        started_at,
        completed_at: Utc::now().to_rfc3339(),
        prompt_version: "v2-communication-gate",
        hotwords_enabled: request.hotwords_enabled,
        force_refinement: request.force_refinement,
        dictionary_terms: dictionary.len(),
        cases: results,
    };
    write_json_atomic(output_path, &result)
}

async fn transcribe(
    provider: &SelectedAsr,
    hints: SpeechRecognitionHints,
    samples: Vec<i16>,
    expected: &str,
) -> AsrResult {
    let started = Instant::now();
    let api_key = provider.api_key.clone();
    let model = provider.descriptor.model.clone();
    let result = tokio::task::spawn_blocking(move || {
        let recognizer = VolcengineSpeechRecognizer::new(template_app::VolcengineAsrSettings {
            enabled: true,
            api_key,
            model,
        })?;
        let session = recognizer.start(hints, Arc::new(|_| {}))?;
        let chunk_count = samples.len().div_ceil(AUDIO_CHUNK_SAMPLES);
        for (index, chunk) in samples.chunks(AUDIO_CHUNK_SAMPLES).enumerate() {
            session.push_audio(chunk.to_vec())?;
            if index + 1 < chunk_count {
                thread::sleep(audio_duration(chunk.len()));
            }
        }
        session.finish()
    })
    .await;
    let (transcript, error) = match result {
        Ok(Ok(text)) => (Some(text), None),
        Ok(Err(error)) => (None, Some(error.to_string())),
        Err(error) => (None, Some(format!("ASR worker failed: {error}"))),
    };
    let metrics = transcript
        .as_deref()
        .map(|text| text_metrics(expected, text));
    AsrResult {
        provider_id: provider.descriptor.id.clone(),
        provider_name: provider.descriptor.name.clone(),
        model: provider.descriptor.model.clone(),
        surface_character_error_rate: metrics.map(|value| value.surface_character_error_rate),
        content_character_error_rate: metrics.map(|value| value.content_character_error_rate),
        punctuation_score: metrics.map(|value| value.punctuation_score),
        transcript,
        error,
        duration_ms: started.elapsed().as_millis(),
    }
}

async fn refine_all(
    providers: &[SelectedLlm],
    transcript: &str,
    case: &EvaluationCase,
    relevant_terms: Vec<template_app::RefinementTerm>,
    force_refinement: bool,
) -> Vec<LlmResult> {
    let language = inferred_language(transcript);
    let mut tasks = JoinSet::new();
    for provider in providers {
        let provider = provider.clone();
        let transcript = transcript.to_owned();
        let expected = case.expected.clone();
        let rules = case.rules.clone();
        let relevant_terms = relevant_terms.clone();
        tasks.spawn(async move {
            let started = Instant::now();
            let mut request = FinalTextRequest::new(transcript, RefinementMode::Enabled);
            request.language = Some(language.to_owned());
            request.relevant_terms = relevant_terms;
            let mode = if force_refinement {
                RefinementEvaluationMode::ForceProvider
            } else {
                RefinementEvaluationMode::ProductionPolicy
            };
            let result = provider
                .processor
                .evaluate(request, CancellationToken::new(), mode)
                .await;
            match result {
                Ok(evaluation) => {
                    let processed = evaluation.processed;
                    let exact_match = processed.text.trim() == expected.trim();
                    let metrics = text_metrics(&expected, &processed.text);
                    let rule_results = evaluate_rules(&rules, &processed.text);
                    LlmResult {
                        provider_id: provider.descriptor.id,
                        provider_name: provider.descriptor.name,
                        model: provider.descriptor.model,
                        surface_character_error_rate: Some(metrics.surface_character_error_rate),
                        content_character_error_rate: Some(metrics.content_character_error_rate),
                        punctuation_score: Some(metrics.punctuation_score),
                        structure_match: Some(metrics.structure_match),
                        rule_pass_rate: pass_rate(&rule_results),
                        rule_results,
                        provider_output: evaluation.provider_output,
                        text: Some(processed.text),
                        status: refinement_status(&processed.refinement),
                        error: None,
                        duration_ms: started.elapsed().as_millis(),
                        exact_match,
                    }
                }
                Err(error) => LlmResult {
                    provider_id: provider.descriptor.id,
                    provider_name: provider.descriptor.name,
                    model: provider.descriptor.model,
                    text: None,
                    provider_output: None,
                    status: "failed".to_owned(),
                    error: Some(error.to_string()),
                    duration_ms: started.elapsed().as_millis(),
                    exact_match: false,
                    surface_character_error_rate: None,
                    content_character_error_rate: None,
                    punctuation_score: None,
                    structure_match: None,
                    rule_results: Vec::new(),
                    rule_pass_rate: None,
                },
            }
        });
    }
    let mut results = Vec::with_capacity(providers.len());
    while let Some(result) = tasks.join_next().await {
        if let Ok(result) = result {
            results.push(result);
        }
    }
    results.sort_by(|left, right| left.provider_name.cmp(&right.provider_name));
    results
}

fn selected_asr(providers: &[ProviderInstance], id: &str) -> Result<SelectedAsr> {
    let provider = providers
        .iter()
        .find(|provider| provider.id == id)
        .with_context(|| format!("ASR provider {id} is unavailable"))?;
    if provider.provider_type != "volcengine" {
        bail!("unsupported ASR provider type: {}", provider.provider_type);
    }
    let api_key = config_string(provider, "api_key")?;
    if api_key.trim().is_empty() {
        bail!("ASR provider is not configured");
    }
    Ok(SelectedAsr {
        descriptor: provider_descriptor(provider),
        api_key,
    })
}

fn selected_llms(providers: &[ProviderInstance], ids: &[String]) -> Result<Vec<SelectedLlm>> {
    ids.iter()
        .map(|id| {
            let provider = providers
                .iter()
                .find(|provider| provider.id == *id)
                .with_context(|| format!("LLM provider {id} is unavailable"))?;
            if provider.provider_type != "openai_compatible" {
                bail!("unsupported LLM provider type: {}", provider.provider_type);
            }
            if provider.data_consent.is_none() {
                bail!("LLM provider {} has no data consent", provider.name);
            }
            let settings = llm_settings(provider)?;
            let implementation = ChatCompletionsLlmProvider::new(settings)?;
            Ok(SelectedLlm {
                descriptor: provider_descriptor(provider),
                processor: Arc::new(FinalTextProcessor::configured(Arc::new(implementation))),
            })
        })
        .collect()
}

fn llm_settings(provider: &ProviderInstance) -> Result<ChatCompletionsLlmSettings> {
    Ok(ChatCompletionsLlmSettings {
        base_url: config_string(provider, "base_url")?,
        api_key: config_string(provider, "api_key")?,
        model: config_string(provider, "model")?,
        custom_headers: provider
            .config
            .get("custom_headers")
            .cloned()
            .map(serde_json::from_value::<BTreeMap<String, String>>)
            .transpose()?
            .unwrap_or_default(),
    })
}

fn provider_descriptor(provider: &ProviderInstance) -> ProviderDescriptor {
    let model = provider
        .config
        .get("model")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let configured = provider
        .config
        .get("api_key")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|key| !key.trim().is_empty())
        && !model.trim().is_empty();
    ProviderDescriptor {
        id: provider.id.clone(),
        name: provider.name.clone(),
        provider_type: provider.provider_type.clone(),
        model,
        configured,
        consented: provider.data_consent.is_some(),
    }
}

fn config_string(provider: &ProviderInstance, field: &str) -> Result<String> {
    provider
        .config
        .get(field)
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
        .with_context(|| format!("{} configuration is missing {field}", provider.name))
}

fn selected_cases<'a>(
    manifest: &'a [EvaluationCase],
    ids: &[String],
) -> Result<Vec<&'a EvaluationCase>> {
    ids.iter()
        .map(|id| {
            manifest
                .iter()
                .find(|case| case.id == *id)
                .with_context(|| format!("evaluation case {id} is unavailable"))
        })
        .collect()
}

fn dictionary_snapshot(environment: AppEnvironment) -> Vec<DictionaryEntry> {
    AppPaths::for_current_user(environment)
        .ok()
        .and_then(|paths| read_dictionary_snapshot(&paths.database()).ok())
        .unwrap_or_default()
}

fn load_catalog(environment: AppEnvironment) -> Result<template_app::ProviderCatalog> {
    Ok(JsonSettingsStore::for_current_user(environment)?.load_catalog()?)
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    serde_json::from_slice(
        &fs::read(path).with_context(|| format!("failed to read {}", path.display()))?,
    )
    .with_context(|| format!("failed to parse {}", path.display()))
}

fn write_progress(
    output_path: &Path,
    completed: usize,
    total: usize,
    current: Option<&str>,
    phase: Option<&str>,
) -> Result<()> {
    let parent = output_path
        .parent()
        .context("evaluation output has no directory")?;
    write_json_atomic(
        &parent.join("progress.json"),
        &Progress {
            completed,
            total,
            current_case: current,
            phase,
        },
    )
}

fn audio_duration(samples: usize) -> Duration {
    Duration::from_secs_f64(samples as f64 / 16_000.0)
}

fn write_json_atomic(path: &Path, value: &impl Serialize) -> Result<()> {
    let parent = path.parent().context("output path has no parent")?;
    fs::create_dir_all(parent)?;
    let temporary = temporary_path(path);
    fs::write(&temporary, serde_json::to_vec_pretty(value)?)?;
    fs::rename(&temporary, path)?;
    Ok(())
}

fn temporary_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".tmp");
    PathBuf::from(name)
}

fn refinement_status(status: &RefinementStatus) -> String {
    match status {
        RefinementStatus::Disabled => "disabled",
        RefinementStatus::Completed => "completed",
        RefinementStatus::Skipped(RefinementSkipReason::ShortTranscript) => "skipped_short",
        RefinementStatus::FellBack(reason) => match reason {
            RefinementFallbackReason::NotConfigured => "fallback_not_configured",
            RefinementFallbackReason::Authentication => "fallback_authentication",
            RefinementFallbackReason::InvalidConfiguration => "fallback_invalid_configuration",
            RefinementFallbackReason::ModelUnavailable => "fallback_model_unavailable",
            RefinementFallbackReason::Quota => "fallback_quota",
            RefinementFallbackReason::Transport => "fallback_transport",
            RefinementFallbackReason::Protocol => "fallback_protocol",
            RefinementFallbackReason::Timeout => "fallback_timeout",
            RefinementFallbackReason::OutputRejected => "fallback_output_rejected",
            RefinementFallbackReason::TemporarilyUnavailable => "fallback_temporarily_unavailable",
        },
    }
    .to_owned()
}

fn inferred_language(text: &str) -> &'static str {
    if text.chars().any(
        |character| matches!(character as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF),
    ) {
        "zh-Hans"
    } else {
        "en"
    }
}

fn parse_environment(value: &str) -> Result<AppEnvironment> {
    match value {
        "development" => Ok(AppEnvironment::Development),
        "production" => Ok(AppEnvironment::Production),
        other => bail!("unsupported environment: {other}"),
    }
}

fn environment_name(environment: AppEnvironment) -> &'static str {
    match environment {
        AppEnvironment::Development => "development",
        AppEnvironment::Production => "production",
    }
}

fn validate_run_id(run_id: &str) -> Result<()> {
    if run_id.is_empty()
        || !run_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        bail!("run id contains unsupported characters");
    }
    Ok(())
}
