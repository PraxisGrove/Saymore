use std::collections::BTreeSet;

use serde::Deserialize;
use thiserror::Error;
use unicode_normalization::UnicodeNormalization;

use crate::{
    CandidateAssessmentSource, CandidateDecision, DictionaryCandidateAssessment,
    DictionaryCandidateKind, DictionaryCorrection, DictionaryLearningOutcome,
    DictionaryLearningStore, HistoryCursor, HistoryStore, LlmProvider, LlmProviderError,
    LlmRefinementRequest, LocalSettings, LocalSettingsStore, NewDictionaryObservation,
    StorageError,
};

const ANALYSIS_INTERVAL_MS: i64 = 24 * 60 * 60 * 1_000;
const HISTORY_PAGE_SIZE: u16 = 50;
const REQUEST_TEXT_BUDGET: usize = 96 * 1_024;
const SAMPLE_PART_BUDGET: usize = 48 * 1_024;
const PART_OVERLAP_CHARS: usize = 32;
const CONSENT_SCOPE_VERSION: &str = "recent-final-text-24h:v1";

pub const VOCABULARY_SUGGESTION_INSTRUCTIONS: &str = r#"You identify reusable personal voice-input dictionary terms from final text that the user actually used. Return only a JSON array. Each item must contain canonical, source_ids, decision, type, and confidence. source_ids must contain only IDs from records where canonical appears as one exact contiguous substring, allowing only ASCII letter casing or full-width/half-width alphanumeric equivalence. Prefer names, brands, products, projects, acronyms, technical or professional terms, and code identifiers. Reject single-character terms, ordinary words, sentence fragments, actions, grammar, punctuation, and generic prose. decision must be accept, reject, or uncertain. type must be named_term, acronym, code_identifier, professional_phrase, ordinary_fragment, or unknown. confidence must be a number from 0 to 1. Do not rewrite or summarize the records."#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VocabularySuggestionSample {
    pub id: String,
    pub dictation_id: String,
    pub text: String,
    pub language: Option<String>,
    pub observed_at_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VocabularySuggestionRunOutcome {
    Skipped,
    Interrupted,
    Completed {
        completed_at_ms: i64,
        dictionary_changed: bool,
    },
}

#[derive(Debug, Error)]
pub enum VocabularySuggestionRunError {
    #[error("vocabulary suggestion storage failed: {0}")]
    Storage(#[from] StorageError),
    #[error("vocabulary suggestion provider failed: {0}")]
    Provider(#[from] LlmProviderError),
}

/// Owns the opt-in state and atomically advances the successful-run checkpoint.
///
/// Implementations must update the checkpoint only while history, the feature,
/// and the supplied consent fingerprint are still active.
pub trait VocabularySuggestionSettingsStore: LocalSettingsStore {
    fn record_vocabulary_suggestion_success(
        &self,
        consent_fingerprint: &str,
        completed_at_ms: i64,
    ) -> Result<bool, StorageError>;
}

pub async fn run_vocabulary_suggestions_if_due(
    provider: &dyn LlmProvider,
    history: &dyn HistoryStore,
    learning: &dyn DictionaryLearningStore,
    settings: &dyn VocabularySuggestionSettingsStore,
    consent_fingerprint: &str,
    now_ms: i64,
) -> Result<VocabularySuggestionRunOutcome, VocabularySuggestionRunError> {
    let current = settings.load_settings()?;
    if !vocabulary_suggestions_due(&current, consent_fingerprint, now_ms) {
        return Ok(VocabularySuggestionRunOutcome::Skipped);
    }
    let samples = recent_vocabulary_samples(history, now_ms.saturating_sub(ANALYSIS_INTERVAL_MS))?;
    let mut dictionary_changed = false;
    for batch in vocabulary_suggestion_batches(samples) {
        if !vocabulary_suggestions_enabled(settings, consent_fingerprint)? {
            return Ok(VocabularySuggestionRunOutcome::Interrupted);
        }
        let observations = identify_vocabulary_suggestions(provider, &batch).await?;
        if !vocabulary_suggestions_enabled(settings, consent_fingerprint)? {
            return Ok(VocabularySuggestionRunOutcome::Interrupted);
        }
        for observation in observations {
            if !vocabulary_suggestions_enabled(settings, consent_fingerprint)? {
                return Ok(VocabularySuggestionRunOutcome::Interrupted);
            }
            dictionary_changed |= matches!(
                learning.record_dictionary_observation(observation)?,
                DictionaryLearningOutcome::Added(_)
            );
        }
    }
    if !settings.record_vocabulary_suggestion_success(consent_fingerprint, now_ms)? {
        return Ok(VocabularySuggestionRunOutcome::Interrupted);
    }
    Ok(VocabularySuggestionRunOutcome::Completed {
        completed_at_ms: now_ms,
        dictionary_changed,
    })
}

pub fn vocabulary_suggestion_consent_fingerprint(provider_id: &str, base_url: &str) -> String {
    let provider_id = provider_id.trim();
    let base_url = base_url.trim().trim_end_matches('/');
    format!(
        "{CONSENT_SCOPE_VERSION}:{}:{provider_id}:{}:{base_url}",
        provider_id.len(),
        base_url.len()
    )
}

pub fn vocabulary_suggestions_due(
    settings: &LocalSettings,
    consent_fingerprint: &str,
    now_ms: i64,
) -> bool {
    settings.history_enabled
        && settings.dictionary_assist_enabled
        && settings.dictionary_assist_consent_fingerprint.as_deref() == Some(consent_fingerprint)
        && settings
            .dictionary_assist_last_success_at_ms
            .is_none_or(|last| now_ms.saturating_sub(last) >= ANALYSIS_INTERVAL_MS)
}

pub fn recent_vocabulary_samples(
    store: &dyn HistoryStore,
    start_at_ms: i64,
) -> Result<Vec<VocabularySuggestionSample>, StorageError> {
    let mut samples = Vec::new();
    let mut cursor: Option<HistoryCursor> = None;
    loop {
        let page = store.history_page(cursor, HISTORY_PAGE_SIZE)?;
        let reached_start = page
            .records
            .last()
            .is_some_and(|record| record.created_at_ms < start_at_ms);
        samples.extend(
            page.records
                .into_iter()
                .filter(|record| record.created_at_ms >= start_at_ms)
                .filter(|record| !record.final_text.trim().is_empty())
                .map(|record| VocabularySuggestionSample {
                    id: record.id.clone(),
                    dictation_id: record.id,
                    text: record.final_text,
                    language: record.language,
                    observed_at_ms: record.created_at_ms,
                }),
        );
        if reached_start || page.next_cursor.is_none() {
            break;
        }
        cursor = page.next_cursor;
    }
    samples.reverse();
    Ok(samples)
}

pub fn vocabulary_suggestion_batches(
    samples: Vec<VocabularySuggestionSample>,
) -> Vec<Vec<VocabularySuggestionSample>> {
    let mut batches = Vec::new();
    let mut batch = Vec::new();
    let mut bytes = 0_usize;
    for sample in samples.into_iter().flat_map(split_sample) {
        let sample_bytes = sample
            .text
            .len()
            .saturating_add(sample.id.len())
            .saturating_add(64);
        if !batch.is_empty() && bytes.saturating_add(sample_bytes) > REQUEST_TEXT_BUDGET {
            batches.push(std::mem::take(&mut batch));
            bytes = 0;
        }
        bytes = bytes.saturating_add(sample_bytes);
        batch.push(sample);
    }
    if !batch.is_empty() {
        batches.push(batch);
    }
    batches
}

pub async fn identify_vocabulary_suggestions(
    provider: &dyn LlmProvider,
    samples: &[VocabularySuggestionSample],
) -> Result<Vec<NewDictionaryObservation>, LlmProviderError> {
    let transcript = serde_json::json!({
        "records": samples.iter().map(|sample| serde_json::json!({
            "id": sample.id,
            "language": sample.language,
            "text": sample.text,
        })).collect::<Vec<_>>()
    })
    .to_string();
    let response = provider
        .refine(LlmRefinementRequest {
            instructions: VOCABULARY_SUGGESTION_INSTRUCTIONS.to_owned(),
            transcript,
            language: None,
            relevant_terms: Vec::new(),
        })
        .await?;
    parse_vocabulary_suggestion_response(&response, samples)
        .map_err(|reason| LlmProviderError::Protocol(reason.to_owned()))
}

pub fn parse_vocabulary_suggestion_response(
    response: &str,
    samples: &[VocabularySuggestionSample],
) -> Result<Vec<NewDictionaryObservation>, &'static str> {
    let items: Vec<VocabularySuggestion> = serde_json::from_str(strip_json_fence(response))
        .map_err(|_| "vocabulary suggestion response is invalid JSON")?;
    let mut observations = Vec::new();
    let mut identities = BTreeSet::new();
    for item in items {
        let canonical = item.canonical.trim();
        if canonical.is_empty() {
            continue;
        }
        let decision = if canonical.chars().count() < 2 {
            CandidateDecision::Reject
        } else {
            item.decision.into_domain()
        };
        let kind = item.kind.into_domain();
        let confidence = confidence(item.confidence)?;
        for source_id in item.source_ids {
            let Some(sample) = samples.iter().find(|sample| sample.id == source_id) else {
                return Err("vocabulary suggestion cited an unknown source ID");
            };
            if !contains_equivalent(&sample.text, canonical) {
                return Err("vocabulary suggestion is absent from its cited source");
            }
            let identity = (sample.dictation_id.clone(), comparison_key(canonical));
            if !identities.insert(identity) {
                continue;
            }
            observations.push(NewDictionaryObservation {
                dictation_id: sample.dictation_id.clone(),
                language: sample.language.clone().unwrap_or_else(|| "und".to_owned()),
                correction: DictionaryCorrection {
                    canonical: canonical.to_owned(),
                },
                assessment: DictionaryCandidateAssessment {
                    decision,
                    kind,
                    confidence,
                    source: CandidateAssessmentSource::VocabularySuggestion,
                },
                observed_at_ms: sample.observed_at_ms,
            });
        }
    }
    Ok(observations)
}

#[derive(Deserialize)]
struct VocabularySuggestion {
    canonical: String,
    source_ids: Vec<String>,
    decision: SuggestionDecision,
    #[serde(rename = "type")]
    kind: SuggestionKind,
    confidence: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum SuggestionDecision {
    Accept,
    Reject,
    Uncertain,
}

impl SuggestionDecision {
    fn into_domain(self) -> CandidateDecision {
        match self {
            Self::Accept => CandidateDecision::Accept,
            Self::Reject => CandidateDecision::Reject,
            Self::Uncertain => CandidateDecision::Uncertain,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum SuggestionKind {
    NamedTerm,
    Acronym,
    CodeIdentifier,
    ProfessionalPhrase,
    OrdinaryFragment,
    Unknown,
}

impl SuggestionKind {
    fn into_domain(self) -> DictionaryCandidateKind {
        match self {
            Self::NamedTerm => DictionaryCandidateKind::NamedTerm,
            Self::Acronym => DictionaryCandidateKind::Acronym,
            Self::CodeIdentifier => DictionaryCandidateKind::CodeIdentifier,
            Self::ProfessionalPhrase => DictionaryCandidateKind::ProfessionalPhrase,
            Self::OrdinaryFragment => DictionaryCandidateKind::OrdinaryFragment,
            Self::Unknown => DictionaryCandidateKind::Unknown,
        }
    }
}

fn split_sample(sample: VocabularySuggestionSample) -> Vec<VocabularySuggestionSample> {
    if sample.text.len() <= SAMPLE_PART_BUDGET {
        return vec![sample];
    }
    let characters = sample.text.chars().collect::<Vec<_>>();
    let mut parts = Vec::new();
    let mut start = 0;
    while start < characters.len() {
        let mut end = start;
        let mut bytes: usize = 0;
        while end < characters.len()
            && bytes.saturating_add(characters[end].len_utf8()) <= SAMPLE_PART_BUDGET
        {
            bytes = bytes.saturating_add(characters[end].len_utf8());
            end += 1;
        }
        parts.push(VocabularySuggestionSample {
            id: format!("{}#part-{}", sample.id, parts.len()),
            dictation_id: sample.dictation_id.clone(),
            text: characters[start..end].iter().collect(),
            language: sample.language.clone(),
            observed_at_ms: sample.observed_at_ms,
        });
        if end == characters.len() {
            break;
        }
        start = end.saturating_sub(PART_OVERLAP_CHARS);
    }
    parts
}

fn vocabulary_suggestions_enabled(
    settings: &dyn LocalSettingsStore,
    consent_fingerprint: &str,
) -> Result<bool, StorageError> {
    settings.load_settings().map(|current| {
        current.history_enabled
            && current.dictionary_assist_enabled
            && current.dictionary_assist_consent_fingerprint.as_deref() == Some(consent_fingerprint)
    })
}

fn confidence(value: f64) -> Result<u8, &'static str> {
    (0.0..=1.0)
        .contains(&value)
        .then(|| (value * 100.0).round() as u8)
        .ok_or("vocabulary suggestion confidence is invalid")
}

fn comparison_key(value: &str) -> String {
    value.trim().nfkc().flat_map(char::to_lowercase).collect()
}

fn contains_equivalent(text: &str, candidate: &str) -> bool {
    comparison_key(text).contains(&comparison_key(candidate))
}

fn strip_json_fence(response: &str) -> &str {
    let trimmed = response.trim();
    trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed)
        .strip_suffix("```")
        .unwrap_or(trimmed)
        .trim()
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use async_trait::async_trait;

    use super::*;

    fn sample(id: &str, text: &str) -> VocabularySuggestionSample {
        VocabularySuggestionSample {
            id: id.to_owned(),
            dictation_id: id.to_owned(),
            text: text.to_owned(),
            language: Some("zh-CN".to_owned()),
            observed_at_ms: 42,
        }
    }

    #[test]
    fn duplicate_sources_only_create_one_observation_per_dictation_and_term() {
        let samples = [sample("one", "我在用通义千问")];
        let response = r#"[{"canonical":"千问","source_ids":["one","one"],"decision":"accept","type":"named_term","confidence":0.91}]"#;

        let observations =
            parse_vocabulary_suggestion_response(response, &samples).unwrap_or_default();

        assert_eq!(1, observations.len());
    }

    #[test]
    fn oversized_text_is_split_with_overlap_and_keeps_the_parent_identity() {
        let mut text = "a".repeat(SAMPLE_PART_BUDGET);
        text.push_str("千问");
        text.push_str(&"b".repeat(64));

        let batches = vocabulary_suggestion_batches(vec![sample("record", &text)]);
        let parts = batches.into_iter().flatten().collect::<Vec<_>>();

        assert!(parts.len() >= 2);
        assert!(parts.iter().all(|part| part.dictation_id == "record"));
        assert!(parts.windows(2).all(|pair| {
            let prefix = pair[1]
                .text
                .chars()
                .take(PART_OVERLAP_CHARS)
                .collect::<String>();
            pair[0].text.ends_with(&prefix)
        }));
    }

    #[test]
    fn single_character_suggestions_are_diagnostic_and_absent_terms_are_rejected() {
        let samples = [sample("one", "我在用通义千问")];
        let single = r#"[{"canonical":"问","source_ids":["one"],"decision":"accept","type":"named_term","confidence":0.99}]"#;
        let observations =
            parse_vocabulary_suggestion_response(single, &samples).unwrap_or_default();
        assert_eq!(1, observations.len());
        assert_eq!(
            CandidateDecision::Reject,
            observations[0].assessment.decision
        );

        let absent = r#"[{"canonical":"OpenAI","source_ids":["one"],"decision":"accept","type":"named_term","confidence":0.91}]"#;
        assert_eq!(
            Err("vocabulary suggestion is absent from its cited source"),
            parse_vocabulary_suggestion_response(absent, &samples)
        );
    }

    #[test]
    fn due_requires_an_exact_scope_and_provider_consent() {
        let fingerprint =
            vocabulary_suggestion_consent_fingerprint("provider-a", "https://a.test/v1/");
        let mut settings = LocalSettings {
            dictionary_assist_enabled: true,
            dictionary_assist_consent_fingerprint: Some(fingerprint.clone()),
            ..LocalSettings::default()
        };

        assert!(vocabulary_suggestions_due(&settings, &fingerprint, 42));
        assert!(!vocabulary_suggestions_due(
            &settings,
            &vocabulary_suggestion_consent_fingerprint("provider-b", "https://a.test/v1"),
            42,
        ));
        settings.history_enabled = false;
        assert!(!vocabulary_suggestions_due(&settings, &fingerprint, 42));
    }

    #[tokio::test]
    async fn disabling_during_a_provider_request_discards_the_response() {
        let fingerprint = vocabulary_suggestion_consent_fingerprint("provider", "https://a.test");
        let settings = Arc::new(FakeSettings::enabled(&fingerprint));
        let learning = FakeLearning::default();
        let provider = DisablingProvider {
            settings: Arc::clone(&settings),
        };

        let outcome = run_vocabulary_suggestions_if_due(
            &provider,
            &FakeHistory::new("我在用千问"),
            &learning,
            settings.as_ref(),
            &fingerprint,
            100,
        )
        .await;

        assert!(matches!(
            outcome,
            Ok(VocabularySuggestionRunOutcome::Interrupted)
        ));
        assert_eq!(0, learning.writes.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn a_later_batch_failure_does_not_advance_the_checkpoint() {
        let fingerprint = vocabulary_suggestion_consent_fingerprint("provider", "https://a.test");
        let settings = FakeSettings::enabled(&fingerprint);
        let provider = FailsAfterFirstRequest::default();
        let history = FakeHistory::new(&"词".repeat(SAMPLE_PART_BUDGET));

        let outcome = run_vocabulary_suggestions_if_due(
            &provider,
            &history,
            &FakeLearning::default(),
            &settings,
            &fingerprint,
            100,
        )
        .await;

        assert!(outcome.is_err());
        assert_eq!(
            None,
            settings
                .load_settings()
                .unwrap_or_default()
                .dictionary_assist_last_success_at_ms
        );
        assert!(provider.requests.load(Ordering::Acquire) >= 2);
    }

    struct FakeSettings(Mutex<LocalSettings>);

    impl FakeSettings {
        fn enabled(fingerprint: &str) -> Self {
            Self(Mutex::new(LocalSettings {
                dictionary_assist_enabled: true,
                dictionary_assist_consent_fingerprint: Some(fingerprint.to_owned()),
                ..LocalSettings::default()
            }))
        }
    }

    impl LocalSettingsStore for FakeSettings {
        fn load_settings(&self) -> Result<LocalSettings, StorageError> {
            self.0
                .lock()
                .map(|settings| settings.clone())
                .map_err(|_| StorageError::Unavailable("fake settings lock poisoned".to_owned()))
        }

        fn save_settings(&self, settings: LocalSettings) -> Result<(), StorageError> {
            self.0
                .lock()
                .map(|mut current| *current = settings)
                .map_err(|_| StorageError::Unavailable("fake settings lock poisoned".to_owned()))
        }
    }

    impl VocabularySuggestionSettingsStore for FakeSettings {
        fn record_vocabulary_suggestion_success(
            &self,
            consent_fingerprint: &str,
            completed_at_ms: i64,
        ) -> Result<bool, StorageError> {
            let mut settings = self
                .0
                .lock()
                .map_err(|_| StorageError::Unavailable("fake settings lock poisoned".to_owned()))?;
            if !vocabulary_suggestions_due(&settings, consent_fingerprint, completed_at_ms) {
                return Ok(false);
            }
            settings.dictionary_assist_last_success_at_ms = Some(completed_at_ms);
            Ok(true)
        }
    }

    struct DisablingProvider {
        settings: Arc<FakeSettings>,
    }

    #[async_trait]
    impl LlmProvider for DisablingProvider {
        async fn refine(&self, _request: LlmRefinementRequest) -> Result<String, LlmProviderError> {
            let mut settings = self
                .settings
                .load_settings()
                .map_err(|error| LlmProviderError::Transport(error.to_string()))?;
            settings.dictionary_assist_enabled = false;
            self.settings
                .save_settings(settings)
                .map_err(|error| LlmProviderError::Transport(error.to_string()))?;
            Ok(r#"[{"canonical":"千问","source_ids":["record"],"decision":"accept","type":"named_term","confidence":0.9}]"#.to_owned())
        }
    }

    #[derive(Default)]
    struct FailsAfterFirstRequest {
        requests: AtomicUsize,
    }

    #[async_trait]
    impl LlmProvider for FailsAfterFirstRequest {
        async fn refine(&self, _request: LlmRefinementRequest) -> Result<String, LlmProviderError> {
            if self.requests.fetch_add(1, Ordering::AcqRel) == 0 {
                Ok("[]".to_owned())
            } else {
                Err(LlmProviderError::Transport("injected failure".to_owned()))
            }
        }
    }

    struct FakeHistory {
        text: String,
    }

    impl FakeHistory {
        fn new(text: &str) -> Self {
            Self {
                text: text.to_owned(),
            }
        }
    }

    impl HistoryStore for FakeHistory {
        fn insert_history(&self, _record: crate::NewHistoryRecord) -> Result<(), StorageError> {
            Ok(())
        }
        fn update_history_final_text(
            &self,
            _id: &str,
            _final_text: &str,
        ) -> Result<(), StorageError> {
            Ok(())
        }
        fn history_page(
            &self,
            cursor: Option<HistoryCursor>,
            _limit: u16,
        ) -> Result<crate::HistoryPage, StorageError> {
            Ok(crate::HistoryPage {
                records: if cursor.is_none() {
                    vec![history_record(&self.text)]
                } else {
                    Vec::new()
                },
                next_cursor: None,
            })
        }
        fn search_history_page(
            &self,
            _cursor: Option<HistoryCursor>,
            _limit: u16,
            _query: &str,
        ) -> Result<crate::HistoryPage, StorageError> {
            self.history_page(None, 1)
        }
        fn update_history_delivery(
            &self,
            _id: &str,
            _delivery: crate::HistoryDelivery,
        ) -> Result<(), StorageError> {
            Ok(())
        }
        fn delete_history(&self, _id: &str) -> Result<(), StorageError> {
            Ok(())
        }
        fn clear_history(&self) -> Result<(), StorageError> {
            Ok(())
        }
        fn reset_history(&self) -> Result<(), StorageError> {
            Ok(())
        }
        fn cleanup_history(&self, _now_ms: i64) -> Result<u64, StorageError> {
            Ok(0)
        }
    }

    fn history_record(text: &str) -> crate::HistoryRecord {
        crate::HistoryRecord {
            id: "record".to_owned(),
            created_at_ms: 99,
            final_text: text.to_owned(),
            raw_asr_text: None,
            llm_refined_text: None,
            audio_duration_ms: 1,
            language: Some("zh-CN".to_owned()),
            delivery: crate::HistoryDelivery::Delivered,
            refinement: crate::HistoryRefinement::NotUsed,
            asr_provider_id: None,
            llm_provider_id: None,
            asr_model: None,
            llm_model: None,
        }
    }

    #[derive(Default)]
    struct FakeLearning {
        writes: AtomicUsize,
    }

    impl DictionaryLearningStore for FakeLearning {
        fn record_dictionary_observation(
            &self,
            _observation: NewDictionaryObservation,
        ) -> Result<DictionaryLearningOutcome, StorageError> {
            self.writes.fetch_add(1, Ordering::AcqRel);
            Ok(DictionaryLearningOutcome::Pending {
                occurrence_count: 1,
                dictation_count: 1,
            })
        }

        fn list_dictionary_candidate_evidence(
            &self,
        ) -> Result<Vec<crate::DictionaryCandidateEvidence>, StorageError> {
            Ok(Vec::new())
        }
    }
}
