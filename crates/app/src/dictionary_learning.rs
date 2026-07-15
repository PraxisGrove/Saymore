use crate::StorageError;

const MAX_CORRECTION_CHARS: usize = 64;
const MAX_CJK_CORRECTION_CHARS: usize = 8;
const MAX_CORRECTION_WORDS: usize = 3;
const MAX_WHOLE_REPLACEMENT_CHARS: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictionaryCorrection {
    pub canonical: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewDictionaryObservation {
    pub dictation_id: String,
    pub language: String,
    pub correction: DictionaryCorrection,
    pub observed_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DictionaryLearningOutcome {
    Pending {
        occurrence_count: u32,
        dictation_count: u32,
    },
    Added(crate::DictionaryEntry),
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
