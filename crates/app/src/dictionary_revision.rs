use std::collections::BTreeMap;

use serde::Deserialize;
use unicode_normalization::UnicodeNormalization;

use crate::{
    CandidateAssessmentSource, CandidateDecision, DictionaryCandidateAssessment,
    DictionaryCandidateKind, DictionaryCandidateReview, DictionaryCorrection, LlmProvider,
    LlmProviderError, LlmRefinementRequest, TextRevisionDiff, review_dictionary_candidate_locally,
};

pub const DICTIONARY_REVISION_INSTRUCTIONS: &str = r#"Identify reusable personal voice-input dictionary terms from a user's final text revision. The input contains one or more independent character-level edits with bounded before/after context. Return only a JSON array. Each item must contain canonical, decision, type, and confidence. canonical must be one exact contiguous substring of an after_context, allowing only ASCII letter casing or full-width/half-width alphanumeric equivalence. Use shared context to return the complete reusable term: for example, when before is 万，after is 问，and after_context contains 千问，return 千问 rather than 问。Return zero or more terms so independent edits can yield separate entries. Prefer names, brands, products, projects, acronyms, technical or professional terms, and code identifiers. Reject single-character terms, ordinary words, sentence fragments, actions, grammar, punctuation, and generic prose. decision must be accept, reject, or uncertain. type must be named_term, acronym, code_identifier, professional_phrase, ordinary_fragment, or unknown. confidence must be a number from 0 to 1. Do not rewrite or summarize the text."#;

pub async fn review_final_text_revision(
    provider: &dyn LlmProvider,
    diffs: &[TextRevisionDiff],
    final_text: &str,
    language: &str,
) -> Result<Vec<DictionaryCandidateReview>, LlmProviderError> {
    let transcript = serde_json::json!({
        "edits": diffs.iter().map(|diff| serde_json::json!({
            "before": diff.before,
            "after": diff.after,
            "before_context": diff.before_context,
            "after_context": diff.after_context,
        })).collect::<Vec<_>>()
    })
    .to_string();
    let response = provider
        .refine(LlmRefinementRequest {
            instructions: DICTIONARY_REVISION_INSTRUCTIONS.to_owned(),
            transcript,
            language: Some(language.to_owned()),
            relevant_terms: Vec::new(),
        })
        .await?;
    parse_dictionary_revision_response(&response, final_text)
        .map_err(|reason| LlmProviderError::Protocol(reason.to_owned()))
}

pub fn local_dictionary_revision_candidates(
    diffs: &[TextRevisionDiff],
) -> Vec<DictionaryCandidateReview> {
    deduplicate_reviews(diffs.iter().filter_map(|diff| {
        let review = review_dictionary_candidate_locally(diff.local_candidate.as_deref()?);
        (review.assessment.decision == CandidateDecision::Accept).then_some(review)
    }))
}

pub fn parse_dictionary_revision_response(
    response: &str,
    final_text: &str,
) -> Result<Vec<DictionaryCandidateReview>, &'static str> {
    let items: Vec<DictionaryRevisionSuggestion> = serde_json::from_str(strip_json_fence(response))
        .map_err(|_| "dictionary revision response is invalid JSON")?;
    let mut reviews = Vec::new();
    for item in items {
        let canonical = item.canonical.trim();
        if canonical.is_empty() || !contains_equivalent(final_text, canonical) {
            continue;
        }
        let decision = if canonical.chars().count() < 2 {
            CandidateDecision::Reject
        } else {
            item.decision.parse()?
        };
        reviews.push(DictionaryCandidateReview {
            correction: DictionaryCorrection {
                canonical: canonical.to_owned(),
            },
            assessment: DictionaryCandidateAssessment {
                decision,
                kind: item.kind.parse()?,
                confidence: confidence(item.confidence)?,
                source: CandidateAssessmentSource::Llm,
            },
        });
    }
    Ok(deduplicate_reviews(reviews))
}

#[derive(Deserialize)]
struct DictionaryRevisionSuggestion {
    canonical: String,
    decision: DecisionValue,
    #[serde(rename = "type")]
    kind: KindValue,
    confidence: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum DecisionValue {
    Accept,
    Reject,
    Uncertain,
}

impl DecisionValue {
    fn parse(self) -> Result<CandidateDecision, &'static str> {
        Ok(match self {
            Self::Accept => CandidateDecision::Accept,
            Self::Reject => CandidateDecision::Reject,
            Self::Uncertain => CandidateDecision::Uncertain,
        })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum KindValue {
    NamedTerm,
    Acronym,
    CodeIdentifier,
    ProfessionalPhrase,
    OrdinaryFragment,
    Unknown,
}

impl KindValue {
    fn parse(self) -> Result<DictionaryCandidateKind, &'static str> {
        Ok(match self {
            Self::NamedTerm => DictionaryCandidateKind::NamedTerm,
            Self::Acronym => DictionaryCandidateKind::Acronym,
            Self::CodeIdentifier => DictionaryCandidateKind::CodeIdentifier,
            Self::ProfessionalPhrase => DictionaryCandidateKind::ProfessionalPhrase,
            Self::OrdinaryFragment => DictionaryCandidateKind::OrdinaryFragment,
            Self::Unknown => DictionaryCandidateKind::Unknown,
        })
    }
}

fn confidence(value: f64) -> Result<u8, &'static str> {
    (0.0..=1.0)
        .contains(&value)
        .then(|| (value * 100.0).round() as u8)
        .ok_or("dictionary revision confidence is invalid")
}

fn deduplicate_reviews(
    reviews: impl IntoIterator<Item = DictionaryCandidateReview>,
) -> Vec<DictionaryCandidateReview> {
    let mut unique: BTreeMap<String, DictionaryCandidateReview> = BTreeMap::new();
    for review in reviews {
        let key = comparison_key(&review.correction.canonical);
        match unique.get(&key) {
            Some(existing) if existing.assessment.confidence >= review.assessment.confidence => {}
            Some(_) | None => {
                unique.insert(key, review);
            }
        }
    }
    unique.into_values().collect()
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
    use super::*;
    use crate::text_revision_diffs;

    #[test]
    fn shared_chinese_context_can_return_the_complete_term() {
        let response =
            r#"[{"canonical":"千问","decision":"accept","type":"named_term","confidence":0.94}]"#;

        let reviews =
            parse_dictionary_revision_response(response, "我在用通义千问模型").unwrap_or_default();

        assert_eq!(1, reviews.len());
        assert_eq!("千问", reviews[0].correction.canonical);
    }

    #[test]
    fn multiple_edits_can_return_multiple_deduplicated_terms() {
        let response = r#"[
            {"canonical":"SlintUI","decision":"accept","type":"named_term","confidence":0.91},
            {"canonical":"JSON","decision":"accept","type":"acronym","confidence":0.95},
            {"canonical":"JSON","decision":"accept","type":"acronym","confidence":0.70}
        ]"#;

        let reviews = parse_dictionary_revision_response(response, "使用 SlintUI 和 JSON")
            .unwrap_or_default();

        assert_eq!(2, reviews.len());
        assert!(
            reviews
                .iter()
                .any(|item| item.correction.canonical == "SlintUI")
        );
        assert!(
            reviews
                .iter()
                .any(|item| item.correction.canonical == "JSON")
        );
    }

    #[test]
    fn local_fallback_only_keeps_unambiguous_ascii_terms() {
        let diffs = text_revision_diffs("使用 slint 和 千万", "使用 SlintUI 和 千问");

        let reviews = local_dictionary_revision_candidates(&diffs);

        assert_eq!(1, reviews.len());
        assert_eq!("SlintUI", reviews[0].correction.canonical);
    }

    #[test]
    fn response_keeps_single_characters_as_diagnostics_and_rejects_absent_terms() {
        let response = r#"[
            {"canonical":"问","decision":"accept","type":"named_term","confidence":0.99},
            {"canonical":"OpenAI","decision":"accept","type":"named_term","confidence":0.99}
        ]"#;

        let reviews = parse_dictionary_revision_response(response, "千问").unwrap_or_default();
        assert_eq!(1, reviews.len());
        assert_eq!(CandidateDecision::Reject, reviews[0].assessment.decision);
    }
}
