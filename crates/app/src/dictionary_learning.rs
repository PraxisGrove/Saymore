use crate::{LlmProvider, LlmProviderError, LlmRefinementRequest, StorageError};
use unicode_normalization::UnicodeNormalization;

const MAX_CORRECTION_CHARS: usize = 64;
const MAX_CJK_CORRECTION_CHARS: usize = 8;
const MAX_CORRECTION_WORDS: usize = 3;
const MAX_WHOLE_REPLACEMENT_CHARS: usize = 32;
const HIGH_CONFIDENCE_THRESHOLD: u8 = 80;

pub const DICTIONARY_CANDIDATE_INSTRUCTIONS: &str = r#"You extract and classify whether a user's local text correction should become a personal voice-input dictionary entry. The canonical value must be the smallest reusable term. It must be an exact contiguous substring of the supplied candidate or differ from one only by ASCII letter casing or full-width/half-width alphanumeric formatting. Use the conventional capitalization when confident; for example, extract "UI" from "ui落实". Never join separate tokens such as "open ai" into "OpenAI". Prefer names, brands, products, projects, acronyms, technical or professional terms, and code identifiers in any language. Reject every single-character candidate because it is too ambiguous for automatic learning. Also reject ordinary sentence fragments, actions, grammar edits, punctuation edits, and generic prose. Return only one JSON object with: canonical (the extracted standard spelling), decision (accept, reject, or uncertain), type (named_term, acronym, code_identifier, professional_phrase, ordinary_fragment, or unknown), and confidence (a number from 0 to 1)."#;

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
    VocabularySuggestion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DictionaryCandidateAssessment {
    pub decision: CandidateDecision,
    pub kind: DictionaryCandidateKind,
    pub confidence: u8,
    pub source: CandidateAssessmentSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictionaryCandidateReview {
    pub correction: DictionaryCorrection,
    pub assessment: DictionaryCandidateAssessment,
}

impl DictionaryCandidateAssessment {
    pub fn required_evidence(self) -> Option<(u32, u32)> {
        match self.decision {
            CandidateDecision::Accept if self.confidence >= HIGH_CONFIDENCE_THRESHOLD => {
                Some((2, 2))
            }
            CandidateDecision::Accept | CandidateDecision::Uncertain if self.confidence >= 60 => {
                Some((3, 3))
            }
            CandidateDecision::Accept | CandidateDecision::Uncertain => None,
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
    if let Some(assessment) = single_character_assessment(canonical) {
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

pub fn review_dictionary_candidate_locally(candidate: &str) -> DictionaryCandidateReview {
    let candidate = candidate.trim();
    let ranges = standard_spelling_token_ranges(candidate);
    let canonical = match ranges.as_slice() {
        [(start, end)] => conventional_dictionary_spelling(&candidate[*start..*end]),
        _ => candidate.to_owned(),
    };
    DictionaryCandidateReview {
        assessment: assess_dictionary_candidate(&canonical),
        correction: DictionaryCorrection { canonical },
    }
}

fn single_character_assessment(canonical: &str) -> Option<DictionaryCandidateAssessment> {
    let mut characters = canonical.chars();
    let _ = characters.next()?;
    characters
        .next()
        .is_none()
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
) -> Result<DictionaryCandidateReview, LlmProviderError> {
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
    parse_dictionary_candidate_review(&response, canonical)
        .map_err(|reason| LlmProviderError::Protocol(reason.to_owned()))
}

pub fn parse_dictionary_candidate_review(
    response: &str,
    candidate: &str,
) -> Result<DictionaryCandidateReview, &'static str> {
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
    let canonical = value
        .get("canonical")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|canonical| {
            eligible_fragment(canonical) && canonical_matches_candidate(candidate, canonical)
        })
        .ok_or("dictionary review has an invalid canonical term")?;
    let canonical = conventional_dictionary_spelling(canonical);
    Ok(DictionaryCandidateReview {
        correction: DictionaryCorrection { canonical },
        assessment: DictionaryCandidateAssessment {
            decision,
            kind,
            confidence: (confidence * 100.0).round() as u8,
            source: CandidateAssessmentSource::Llm,
        },
    })
}

fn canonical_matches_candidate(candidate: &str, canonical: &str) -> bool {
    if candidate.contains(canonical) {
        return true;
    }
    let Some(canonical_key) = standard_spelling_key(canonical) else {
        return false;
    };
    standard_spelling_token_ranges(candidate)
        .into_iter()
        .any(|(start, end)| {
            standard_spelling_key(&candidate[start..end]).as_deref() == Some(canonical_key.as_str())
        })
}

fn standard_spelling_key(value: &str) -> Option<String> {
    let normalized = value.trim().nfkc().collect::<String>();
    (normalized.chars().count() >= 2
        && normalized
            .chars()
            .all(|character| character.is_ascii_alphanumeric()))
    .then(|| normalized.to_ascii_lowercase())
}

fn is_standard_spelling_character(character: char) -> bool {
    character.is_ascii_alphanumeric()
        || matches!(character as u32, 0xFF10..=0xFF19 | 0xFF21..=0xFF3A | 0xFF41..=0xFF5A)
}

fn standard_spelling_token_ranges(value: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut token_start = None;
    for (index, character) in value.char_indices() {
        if is_standard_spelling_character(character) {
            token_start.get_or_insert(index);
        } else if let Some(start) = token_start.take() {
            ranges.push((start, index));
        }
    }
    if let Some(start) = token_start {
        ranges.push((start, value.len()));
    }
    ranges
}

fn conventional_dictionary_spelling(value: &str) -> String {
    let normalized = value.trim().nfkc().collect::<String>();
    let Some(key) = standard_spelling_key(&normalized) else {
        return normalized;
    };
    match key.as_str() {
        "ai" => "AI",
        "api" => "API",
        "asr" => "ASR",
        "cli" => "CLI",
        "cpu" => "CPU",
        "css" => "CSS",
        "github" => "GitHub",
        "gpu" => "GPU",
        "gui" => "GUI",
        "html" => "HTML",
        "http" => "HTTP",
        "https" => "HTTPS",
        "ide" => "IDE",
        "json" => "JSON",
        "llm" => "LLM",
        "openai" => "OpenAI",
        "ram" => "RAM",
        "sdk" => "SDK",
        "sql" => "SQL",
        "ui" => "UI",
        "uri" => "URI",
        "url" => "URL",
        "uuid" => "UUID",
        "ux" => "UX",
        "xml" => "XML",
        _ => return normalized,
    }
    .to_owned()
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
    AlreadyPresent,
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
    fn rejects_every_single_character_from_automatic_learning() {
        let expected = DictionaryCandidateAssessment {
            decision: CandidateDecision::Reject,
            kind: DictionaryCandidateKind::Unknown,
            confidence: 100,
            source: CandidateAssessmentSource::Local,
        };

        for candidate in ["n", "N", "三", "问"] {
            assert_eq!(expected, assess_dictionary_candidate(candidate));
        }
    }

    #[test]
    fn high_confidence_candidates_need_two_independent_corrections() {
        let assessment = assess_dictionary_candidate("Vercel");

        assert_eq!(Some((2, 2)), assessment.required_evidence());
    }

    #[test]
    fn medium_confidence_candidates_need_three_independent_dictations() {
        let assessment = DictionaryCandidateAssessment {
            decision: CandidateDecision::Accept,
            kind: DictionaryCandidateKind::Unknown,
            confidence: 79,
            source: CandidateAssessmentSource::Llm,
        };

        assert_eq!(Some((3, 3)), assessment.required_evidence());
    }

    #[test]
    fn high_confidence_vocabulary_suggestions_need_two_independent_dictations() {
        let assessment = DictionaryCandidateAssessment {
            decision: CandidateDecision::Accept,
            kind: DictionaryCandidateKind::NamedTerm,
            confidence: 99,
            source: CandidateAssessmentSource::VocabularySuggestion,
        };

        assert_eq!(Some((2, 2)), assessment.required_evidence());
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
            Ok(DictionaryCandidateReview {
                correction: DictionaryCorrection {
                    canonical: "逆地理编码".to_owned(),
                },
                assessment: DictionaryCandidateAssessment {
                    decision: CandidateDecision::Accept,
                    kind: DictionaryCandidateKind::ProfessionalPhrase,
                    confidence: 93,
                    source: CandidateAssessmentSource::Llm,
                },
            }),
            parse_dictionary_candidate_review(
                r#"{"canonical":"逆地理编码","decision":"accept","type":"professional_phrase","confidence":0.93}"#,
                "逆地理编码"
            )
        );
        assert!(parse_dictionary_candidate_review("not json", "逆地理编码").is_err());
        assert!(
            parse_dictionary_candidate_review(
                r#"{"canonical":"逆地理编码","decision":"accept","type":"professional_phrase","confidence":2}"#,
                "逆地理编码"
            )
            .is_err()
        );
    }

    #[test]
    fn llm_review_standardizes_a_reusable_term_from_a_mixed_edit_fragment() {
        let expected = Ok(DictionaryCandidateReview {
            correction: DictionaryCorrection {
                canonical: "UI".to_owned(),
            },
            assessment: DictionaryCandidateAssessment {
                decision: CandidateDecision::Accept,
                kind: DictionaryCandidateKind::Acronym,
                confidence: 90,
                source: CandidateAssessmentSource::Llm,
            },
        });
        let response =
            r#"{"canonical":"UI","decision":"accept","type":"acronym","confidence":0.9}"#;

        for candidate in ["ui 落实", "UI 落实"] {
            assert_eq!(
                expected,
                parse_dictionary_candidate_review(response, candidate)
            );
        }
    }

    #[test]
    fn llm_review_rejects_a_canonical_term_absent_from_the_edit_fragment() {
        assert_eq!(
            Err("dictionary review has an invalid canonical term"),
            parse_dictionary_candidate_review(
                r#"{"canonical":"OpenAI","decision":"accept","type":"named_term","confidence":0.9}"#,
                "ui 落实",
            )
        );
        assert!(
            parse_dictionary_candidate_review(
                r#"{"canonical":"OpenAI","decision":"accept","type":"named_term","confidence":0.9}"#,
                "open ai",
            )
            .is_err()
        );
    }

    #[test]
    fn local_review_extracts_and_standardizes_one_mixed_script_term() {
        let cases = [
            ("ui 落实", "UI", DictionaryCandidateKind::Acronym, 90),
            ("UI 落实", "UI", DictionaryCandidateKind::Acronym, 90),
            ("API 接口", "API", DictionaryCandidateKind::Acronym, 90),
            (
                "使用 OpenAI 模型",
                "OpenAI",
                DictionaryCandidateKind::NamedTerm,
                86,
            ),
            ("wiki 百科", "wiki", DictionaryCandidateKind::Unknown, 45),
        ];

        for (candidate, canonical, kind, confidence) in cases {
            let review = review_dictionary_candidate_locally(candidate);
            assert_eq!(canonical, review.correction.canonical);
            assert_eq!(kind, review.assessment.kind);
            assert_eq!(confidence, review.assessment.confidence);
            assert_eq!(CandidateAssessmentSource::Local, review.assessment.source);
        }
    }

    #[test]
    fn local_review_keeps_multiple_tokens_and_chinese_phrases_intact() {
        for candidate in ["open ai 落实", "UI 和 UX", "路径渲染"] {
            assert_eq!(
                candidate,
                review_dictionary_candidate_locally(candidate)
                    .correction
                    .canonical
            );
        }
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
