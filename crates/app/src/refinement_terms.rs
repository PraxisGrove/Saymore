use std::{collections::BTreeMap, ops::Range};

use unicode_normalization::UnicodeNormalization;

use crate::{
    final_text_processing::RefinementTerm,
    storage::{DictionaryEntry, DictionaryStore, StorageError, normalize_language_tag},
};

const MAX_RELEVANT_TERMS: usize = 50;

pub fn relevant_dictionary_terms(
    store: &dyn DictionaryStore,
    transcript: &str,
    language: &str,
) -> Result<Vec<RefinementTerm>, StorageError> {
    let language = normalize_language_tag(language)?;
    Ok(select_relevant_terms(
        store.list_dictionary()?,
        transcript,
        &language,
    ))
}

pub fn normalize_standard_spellings(text: &str, terms: &[RefinementTerm]) -> String {
    let spellings = terms
        .iter()
        .filter_map(|term| {
            standard_spelling_key(&term.canonical)
                .map(|key| (key, term.canonical.trim().to_owned()))
        })
        .collect::<BTreeMap<_, _>>();
    if spellings.is_empty() {
        return text.to_owned();
    }

    let mut normalized = String::with_capacity(text.len());
    let mut copied_until = 0;
    for range in spelling_token_ranges(text) {
        normalized.push_str(&text[copied_until..range.start]);
        let token = &text[range.clone()];
        let replacement = standard_spelling_key(token)
            .filter(|_| !is_protected_token(text, range.start, range.end))
            .and_then(|key| spellings.get(&key));
        normalized.push_str(replacement.map_or(token, String::as_str));
        copied_until = range.end;
    }
    normalized.push_str(&text[copied_until..]);
    normalized
}

pub fn standard_spelling_occurs(text: &str, canonical: &str) -> bool {
    let Some(expected) = standard_spelling_key(canonical) else {
        return false;
    };
    spelling_token_ranges(text).any(|range| {
        !is_protected_token(text, range.start, range.end)
            && standard_spelling_key(&text[range]).as_deref() == Some(expected.as_str())
    })
}

fn select_relevant_terms(
    entries: Vec<DictionaryEntry>,
    transcript: &str,
    language: &str,
) -> Vec<RefinementTerm> {
    entries
        .into_iter()
        .filter(|entry| entry.language == language)
        .filter(|entry| standard_spelling_occurs(transcript, &entry.canonical))
        .take(MAX_RELEVANT_TERMS)
        .map(|entry| RefinementTerm {
            canonical: entry.canonical,
        })
        .collect()
}

fn standard_spelling_key(value: &str) -> Option<String> {
    let normalized = value.trim().nfkc().collect::<String>();
    (normalized.chars().count() >= 2
        && normalized
            .chars()
            .all(|character| character.is_ascii_alphanumeric()))
    .then(|| normalized.to_ascii_lowercase())
}

fn is_spelling_character(character: char) -> bool {
    character.is_ascii_alphanumeric()
        || matches!(character as u32, 0xFF10..=0xFF19 | 0xFF21..=0xFF3A | 0xFF41..=0xFF5A)
}

fn spelling_token_ranges(text: &str) -> impl Iterator<Item = Range<usize>> + '_ {
    let mut index = 0;
    std::iter::from_fn(move || {
        while index < text.len() {
            let character = text[index..].chars().next()?;
            if is_spelling_character(character) {
                break;
            }
            index += character.len_utf8();
        }
        if index == text.len() {
            return None;
        }
        let start = index;
        while index < text.len() {
            let character = text[index..].chars().next()?;
            if !is_spelling_character(character) {
                break;
            }
            index += character.len_utf8();
        }
        Some(start..index)
    })
}

fn is_protected_token(text: &str, start: usize, end: usize) -> bool {
    let container_start = text[..start]
        .rfind(char::is_whitespace)
        .map_or(0, |index| index + 1);
    let container_end = text[end..]
        .find(char::is_whitespace)
        .map_or(text.len(), |index| end + index);
    let container = &text[container_start..container_end];
    container.contains("://")
        || container.contains('/')
        || container.contains('\\')
        || container.contains('@')
        || text[..start].ends_with('_')
        || text[end..].starts_with('_')
        || joins_domain_label(text, start, end)
}

fn joins_domain_label(text: &str, start: usize, end: usize) -> bool {
    let preceded_by_label = text[..start]
        .strip_suffix('.')
        .and_then(|prefix| prefix.chars().next_back())
        .is_some_and(char::is_alphanumeric);
    let followed_by_label = text[end..]
        .strip_prefix('.')
        .and_then(|suffix| suffix.chars().next())
        .is_some_and(char::is_alphanumeric);
    preceded_by_label || followed_by_label
}

#[cfg(test)]
mod tests {
    use crate::{DictionaryOrigin, storage::DictionaryEntry};

    use super::*;

    #[test]
    fn relevant_terms_are_isolated_by_language_and_ignore_legacy_variants() {
        let entries = vec![
            entry("en", "OpenAI", vec!["open ai"]),
            entry("zh-Hans", "OPENAI", vec!["欧盆AI"]),
            entry("zh-Hans", "SQLite", Vec::new()),
        ];

        assert_eq!(
            vec![RefinementTerm {
                canonical: "OPENAI".to_owned(),
            }],
            select_relevant_terms(entries, "请使用 openai", "zh-Hans")
        );
    }

    fn entry(language: &str, canonical: &str, variants: Vec<&str>) -> DictionaryEntry {
        DictionaryEntry {
            id: format!("{language}-{canonical}"),
            canonical: canonical.to_owned(),
            language: language.to_owned(),
            variants: variants.into_iter().map(str::to_owned).collect(),
            origin: DictionaryOrigin::Manual,
            created_at_ms: 1,
            updated_at_ms: 1,
        }
    }
}
