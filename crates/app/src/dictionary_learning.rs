use crate::{LlmProvider, LlmProviderError, LlmRefinementRequest, StorageError};

const MAX_CORRECTION_CHARS: usize = 64;
const MAX_CJK_CORRECTION_CHARS: usize = 8;
const MAX_CORRECTION_WORDS: usize = 3;
const MAX_WHOLE_REPLACEMENT_CHARS: usize = 32;
const HIGH_CONFIDENCE_THRESHOLD: u8 = 80;

pub const DICTIONARY_CANDIDATE_INSTRUCTIONS: &str = r#"You classify whether a user's local text correction should become a personal voice-input dictionary entry. Prefer names, brands, products, projects, acronyms, technical or professional terms, and code identifiers in any language. Reject single-character ASCII letter candidates because they are too ambiguous for automatic learning. Also reject ordinary sentence fragments, actions, grammar edits, punctuation edits, and generic prose. Return only one JSON object with: decision (accept, reject, or uncertain), type (named_term, acronym, code_identifier, professional_phrase, ordinary_fragment, or unknown), and confidence (a number from 0 to 1)."#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateDecision {
    Accept,
    Reject,
    Uncertain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DictionaryCandidateKind {
    NamedTerm,
    Acronym,
    CodeIdentifier,
    ProfessionalPhrase,
    OrdinaryFragment,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateAssessmentSource {
    Local,
    Llm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DictionaryCandidateAssessment {
    pub decision: CandidateDecision,
    pub kind: DictionaryCandidateKind,
    pub confidence: u8,
    pub source: CandidateAssessmentSource,
}

impl DictionaryCandidateAssessment {
    pub fn required_evidence(self) -> Option<(u32, u32)> {
        match self.decision {
            CandidateDecision::Accept if self.confidence >= HIGH_CONFIDENCE_THRESHOLD => {
                Some((2, 2))
            }
            CandidateDecision::Accept => Some((5, 3)),
            CandidateDecision::Uncertain => Some((5, 3)),
            CandidateDecision::Reject => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictionaryCorrection {
    pub canonical: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewDictionaryObservation {
    pub dictation_id: String,
    pub language: String,
    pub correction: DictionaryCorrection,
    pub assessment: DictionaryCandidateAssessment,
    pub observed_at_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DictionaryCandidateState {
    Pending,
    Promoted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictionaryCandidateEvidence {
    pub canonical: String,
    pub language: String,
    pub assessment: DictionaryCandidateAssessment,
    pub occurrence_count: u32,
    pub dictation_count: u32,
    pub state: DictionaryCandidateState,
    pub last_observed_at_ms: i64,
}

pub fn assess_dictionary_candidate(canonical: &str) -> DictionaryCandidateAssessment {
    let canonical = canonical.trim();
    if let Some(assessment) = single_ascii_letter_assessment(canonical) {
        return assessment;
    }
    let chars = canonical.chars().collect::<Vec<_>>();
    let ascii_token = !canonical.is_empty()
        && canonical.is_ascii()
        && canonical
            .chars()
            .all(|character| character.is_ascii_alphanumeric());
    let all_upper = ascii_token
        && chars
            .iter()
            .any(|character| character.is_ascii_alphabetic())
        && chars
            .iter()
            .filter(|character| character.is_ascii_alphabetic())
            .all(|character| character.is_ascii_uppercase());
    let code_identifier = ascii_token
        && chars
            .first()
            .is_some_and(|character| character.is_ascii_lowercase())
        && chars
            .iter()
            .skip(1)
            .any(|character| character.is_ascii_uppercase());
    let named_term = ascii_token
        && chars
            .first()
            .is_some_and(|character| character.is_ascii_uppercase())
        && chars
            .iter()
            .skip(1)
            .any(|character| character.is_ascii_lowercase());
    let (decision, kind, confidence) = if all_upper && chars.len() >= 2 {
        (
            CandidateDecision::Accept,
            DictionaryCandidateKind::Acronym,
            90,
        )
    } else if code_identifier {
        (
            CandidateDecision::Accept,
            DictionaryCandidateKind::CodeIdentifier,
            94,
        )
    } else if named_term {
        (
            CandidateDecision::Accept,
            DictionaryCandidateKind::NamedTerm,
            86,
        )
    } else if canonical.chars().any(is_cjk) && looks_like_ordinary_fragment(canonical) {
        (
            CandidateDecision::Reject,
            DictionaryCandidateKind::OrdinaryFragment,
            92,
        )
    } else if canonical.chars().any(is_cjk) {
        (
            CandidateDecision::Uncertain,
            DictionaryCandidateKind::ProfessionalPhrase,
            62,
        )
    } else {
        (
            CandidateDecision::Uncertain,
            DictionaryCandidateKind::Unknown,
            45,
        )
    };
    DictionaryCandidateAssessment {
        decision,
        kind,
        confidence,
        source: CandidateAssessmentSource::Local,
    }
}

fn single_ascii_letter_assessment(canonical: &str) -> Option<DictionaryCandidateAssessment> {
    let [character] = canonical.as_bytes() else {
        return None;
    };
    character
        .is_ascii_alphabetic()
        .then_some(DictionaryCandidateAssessment {
            decision: CandidateDecision::Reject,
            kind: DictionaryCandidateKind::Unknown,
            confidence: 100,
            source: CandidateAssessmentSource::Local,
        })
}

pub async fn review_dictionary_candidate(
    provider: &dyn LlmProvider,
    canonical: &str,
    original_fragment: &str,
    edited_fragment: &str,
    language: &str,
) -> Result<DictionaryCandidateAssessment, LlmProviderError> {
    let transcript = serde_json::json!({
        "candidate": canonical,
        "before": original_fragment,
        "after": edited_fragment,
    })
    .to_string();
    let response = provider
        .refine(LlmRefinementRequest {
            instructions: DICTIONARY_CANDIDATE_INSTRUCTIONS.to_owned(),
            transcript,
            language: Some(language.to_owned()),
            relevant_terms: Vec::new(),
        })
        .await?;
    parse_dictionary_candidate_review(&response)
        .map_err(|reason| LlmProviderError::Protocol(reason.to_owned()))
}

pub fn parse_dictionary_candidate_review(
    response: &str,
) -> Result<DictionaryCandidateAssessment, &'static str> {
    let trimmed = response.trim();
    let json = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed)
        .strip_suffix("```")
        .unwrap_or(trimmed)
        .trim();
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|_| "dictionary review is invalid JSON")?;
    let decision = match value.get("decision").and_then(serde_json::Value::as_str) {
        Some("accept") => CandidateDecision::Accept,
        Some("reject") => CandidateDecision::Reject,
        Some("uncertain") => CandidateDecision::Uncertain,
        _ => return Err("dictionary review has an invalid decision"),
    };
    let kind = match value.get("type").and_then(serde_json::Value::as_str) {
        Some("named_term") => DictionaryCandidateKind::NamedTerm,
        Some("acronym") => DictionaryCandidateKind::Acronym,
        Some("code_identifier") => DictionaryCandidateKind::CodeIdentifier,
        Some("professional_phrase") => DictionaryCandidateKind::ProfessionalPhrase,
        Some("ordinary_fragment") => DictionaryCandidateKind::OrdinaryFragment,
        Some("unknown") => DictionaryCandidateKind::Unknown,
        _ => return Err("dictionary review has an invalid type"),
    };
    let confidence = value
        .get("confidence")
        .and_then(serde_json::Value::as_f64)
        .filter(|confidence| (0.0..=1.0).contains(confidence))
        .ok_or("dictionary review has an invalid confidence")?;
    Ok(DictionaryCandidateAssessment {
        decision,
        kind,
        confidence: (confidence * 100.0).round() as u8,
        source: CandidateAssessmentSource::Llm,
    })
}

fn looks_like_ordinary_fragment(value: &str) -> bool {
    const ORDINARY_PREFIXES: [&str; 11] = [
        "要求", "需要", "进行", "修改", "然后", "帮我", "可以", "应该", "新增", "删除", "添加",
    ];
    const ORDINARY_MARKERS: [&str; 13] = [
        "我", "你", "他", "这", "那", "很", "了", "的", "吗", "吧", "呢", "请", "帮",
    ];
    ORDINARY_PREFIXES
        .iter()
        .any(|prefix| value.starts_with(prefix))
        || ORDINARY_MARKERS.iter().any(|marker| value.contains(marker))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DictionaryLearningOutcome {
    Pending {
        occurrence_count: u32,
        dictation_count: u32,
    },
    Added(crate::DictionaryEntry),
    Rejected,
    Suppressed,
}

/// Accumulates local correction evidence and promotes repeated corrections to confirmed entries.
///
/// Implementations are expected to keep full surrounding text out of durable storage, count
/// independent dictations separately, and honor suppression state before creating an entry.
pub trait DictionaryLearningStore: Send + Sync {
    fn record_dictionary_observation(
        &self,
        observation: NewDictionaryObservation,
    ) -> Result<DictionaryLearningOutcome, StorageError>;

    fn list_dictionary_candidate_evidence(
        &self,
    ) -> Result<Vec<DictionaryCandidateEvidence>, StorageError>;
}

pub fn correction_from_edit(original: &str, edited: &str) -> Option<DictionaryCorrection> {
    if original == edited {
        return None;
    }
    let original = original.chars().collect::<Vec<_>>();
    let edited = edited.chars().collect::<Vec<_>>();
    let prefix = common_prefix_len(&original, &edited);
    let suffix = common_suffix_len(&original[prefix..], &edited[prefix..]);
    let recognized_as = original[prefix..original.len().saturating_sub(suffix)]
        .iter()
        .collect::<String>();
    let canonical = edited[prefix..edited.len().saturating_sub(suffix)]
        .iter()
        .collect::<String>();
    let recognized_as = recognized_as.trim();
    let canonical = canonical.trim();
    if !eligible_fragment(recognized_as) || !eligible_fragment(canonical) {
        return None;
    }
    if suffix == 0
        && canonical.split_whitespace().count() > recognized_as.split_whitespace().count()
    {
        return None;
    }
    let replaces_entire_text = prefix == 0 && suffix == 0;
    if replaces_entire_text
        && (recognized_as.chars().count() > MAX_WHOLE_REPLACEMENT_CHARS
            || canonical.chars().count() > MAX_WHOLE_REPLACEMENT_CHARS
            || recognized_as.split_whitespace().count() > 1
            || canonical.split_whitespace().count() > 1)
    {
        return None;
    }
    Some(DictionaryCorrection {
        canonical: canonical.to_owned(),
    })
}

fn common_prefix_len(left: &[char], right: &[char]) -> usize {
    left.iter()
        .zip(right)
        .take_while(|(left, right)| left == right)
        .count()
}

fn common_suffix_len(left: &[char], right: &[char]) -> usize {
    left.iter()
        .rev()
        .zip(right.iter().rev())
        .take_while(|(left, right)| left == right)
        .count()
}

fn eligible_fragment(value: &str) -> bool {
    let char_count = value.chars().count();
    !value.is_empty()
        && char_count <= MAX_CORRECTION_CHARS
        && value.split_whitespace().count() <= MAX_CORRECTION_WORDS
        && !value.contains(['\n', '\r'])
        && value.chars().any(char::is_alphanumeric)
        && (!value.chars().any(is_cjk) || char_count <= MAX_CJK_CORRECTION_CHARS)
}

fn is_cjk(character: char) -> bool {
    matches!(
        character,
        '\u{3400}'..='\u{4dbf}'
            | '\u{4e00}'..='\u{9fff}'
            | '\u{f900}'..='\u{faff}'
            | '\u{3040}'..='\u{30ff}'
            | '\u{ac00}'..='\u{d7af}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_personal_terms_without_an_llm() {
        let cases = [
            (
                "Vercel",
                DictionaryCandidateKind::NamedTerm,
                CandidateDecision::Accept,
            ),
            (
                "Versa",
                DictionaryCandidateKind::NamedTerm,
                CandidateDecision::Accept,
            ),
            (
                "POI",
                DictionaryCandidateKind::Acronym,
                CandidateDecision::Accept,
            ),
            (
                "immersiveLayoutHeight",
                DictionaryCandidateKind::CodeIdentifier,
                CandidateDecision::Accept,
            ),
            (
                "逆地理编码",
                DictionaryCandidateKind::ProfessionalPhrase,
                CandidateDecision::Uncertain,
            ),
            (
                "地理编码",
                DictionaryCandidateKind::ProfessionalPhrase,
                CandidateDecision::Uncertain,
            ),
            (
                "路径渲染",
                DictionaryCandidateKind::ProfessionalPhrase,
                CandidateDecision::Uncertain,
            ),
        ];

        for (canonical, kind, decision) in cases {
            let assessment = assess_dictionary_candidate(canonical);
            assert_eq!((kind, decision), (assessment.kind, assessment.decision));
        }
    }

    #[test]
    fn rejects_single_ascii_letters_from_automatic_learning() {
        let expected = DictionaryCandidateAssessment {
            decision: CandidateDecision::Reject,
            kind: DictionaryCandidateKind::Unknown,
            confidence: 100,
            source: CandidateAssessmentSource::Local,
        };

        for candidate in ["n", "N"] {
            assert_eq!(expected, assess_dictionary_candidate(candidate));
        }
    }

    #[test]
    fn high_confidence_candidates_need_two_independent_corrections() {
        let assessment = assess_dictionary_candidate("Vercel");

        assert_eq!(Some((2, 2)), assessment.required_evidence());
    }

    #[test]
    fn low_confidence_acceptance_still_needs_repeated_evidence() {
        let assessment = DictionaryCandidateAssessment {
            decision: CandidateDecision::Accept,
            kind: DictionaryCandidateKind::Unknown,
            confidence: 79,
            source: CandidateAssessmentSource::Llm,
        };

        assert_eq!(Some((5, 3)), assessment.required_evidence());
    }

    #[test]
    fn rejects_an_ordinary_sentence_fragment() {
        for fragment in ["要求后续变更", "今天天气很好", "我觉得可以", "这个需要修改"]
        {
            assert_eq!(
                CandidateDecision::Reject,
                assess_dictionary_candidate(fragment).decision,
                "{fragment} should not become a dictionary candidate"
            );
        }
    }

    #[test]
    fn parses_structured_llm_candidate_reviews() {
        assert_eq!(
            Ok(DictionaryCandidateAssessment {
                decision: CandidateDecision::Accept,
                kind: DictionaryCandidateKind::ProfessionalPhrase,
                confidence: 93,
                source: CandidateAssessmentSource::Llm,
            }),
            parse_dictionary_candidate_review(
                r#"{"decision":"accept","type":"professional_phrase","confidence":0.93}"#
            )
        );
        assert!(parse_dictionary_candidate_review("not json").is_err());
        assert!(
            parse_dictionary_candidate_review(
                r#"{"decision":"accept","type":"professional_phrase","confidence":2}"#
            )
            .is_err()
        );
    }

    #[test]
    fn extracts_one_local_word_replacement() {
        assert_eq!(
            Some(DictionaryCorrection {
                canonical: "Saymore".to_owned(),
            }),
            correction_from_edit("我们使用 CM 开发", "我们使用 Saymore 开发")
        );
    }

    #[test]
    fn keeps_standard_spelling_corrections() {
        assert_eq!(
            Some(DictionaryCorrection {
                canonical: "OpenAI".to_owned(),
            }),
            correction_from_edit("使用 open ai", "使用 OpenAI")
        );
    }

    #[test]
    fn rejects_continuation_deletion_and_punctuation_only_edits() {
        assert_eq!(
            None,
            correction_from_edit("使用 Saymore", "使用 Saymore 开发")
        );
        assert_eq!(None, correction_from_edit("使用 Saymore", "使用"));
        assert_eq!(None, correction_from_edit("你好，世界", "你好。世界"));
        assert_eq!(None, correction_from_edit("使用 CM", "使用 Saymore 开发"));
    }

    #[test]
    fn rejects_whole_sentence_rewrites() {
        assert_eq!(
            None,
            correction_from_edit("明天下午讨论登录问题", "明天下午三点召开登录系统评审会议")
        );
    }
}
