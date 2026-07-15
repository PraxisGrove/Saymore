use std::{cmp::Reverse, ops::Range};

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
    relevant_dictionary_terms_from_entries(store.list_dictionary()?, transcript, language)
}

pub fn relevant_dictionary_terms_from_entries(
    entries: Vec<DictionaryEntry>,
    transcript: &str,
    language: &str,
) -> Result<Vec<RefinementTerm>, StorageError> {
    let language = normalize_language_tag(language)?;
    Ok(select_relevant_terms(entries, transcript, &language))
}

pub fn normalize_standard_spellings(text: &str, terms: &[RefinementTerm]) -> String {
    let mut replacements = terms
        .iter()
        .flat_map(|term| {
            spelling_match_ranges(text, &term.canonical)
                .into_iter()
                .map(|range| Replacement {
                    range,
                    canonical: term.canonical.trim(),
                })
        })
        .collect::<Vec<_>>();
    replacements.sort_by_key(|replacement| {
        (
            replacement.range.start,
            Reverse(replacement.range.end - replacement.range.start),
        )
    });

    let mut normalized = String::with_capacity(text.len());
    let mut copied_until = 0;
    for replacement in replacements {
        if replacement.range.start < copied_until {
            continue;
        }
        normalized.push_str(&text[copied_until..replacement.range.start]);
        normalized.push_str(replacement.canonical);
        copied_until = replacement.range.end;
    }
    normalized.push_str(&text[copied_until..]);
    normalized
}

pub fn standard_spelling_occurs(text: &str, spelling: &str) -> bool {
    !spelling_match_ranges(text, spelling).is_empty()
}

fn select_relevant_terms(
    entries: Vec<DictionaryEntry>,
    transcript: &str,
    language: &str,
) -> Vec<RefinementTerm> {
    entries
        .into_iter()
        .filter(|entry| entry.language == language || spelling_pattern(&entry.canonical).is_some())
        .filter_map(|entry| {
            standard_spelling_occurs(transcript, &entry.canonical).then_some(RefinementTerm {
                canonical: entry.canonical,
            })
        })
        .take(MAX_RELEVANT_TERMS)
        .collect()
}

struct Replacement<'a> {
    range: Range<usize>,
    canonical: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpellingSeparator {
    Space,
    Hyphen,
}

#[derive(Debug, PartialEq, Eq)]
struct SpellingPattern {
    tokens: Vec<String>,
    separators: Vec<SpellingSeparator>,
}

fn spelling_match_ranges(text: &str, spelling: &str) -> Vec<Range<usize>> {
    let Some(expected) = spelling_pattern(spelling) else {
        return Vec::new();
    };
    let actual = spelling_token_ranges(text)
        .filter_map(|range| standard_spelling_key(&text[range.clone()]).map(|key| (range, key)))
        .collect::<Vec<_>>();
    if actual.len() < expected.tokens.len() {
        return Vec::new();
    }

    actual
        .windows(expected.tokens.len())
        .filter_map(|window| {
            let tokens_match = window.iter().map(|(_, key)| key).eq(expected.tokens.iter());
            let separators_match = window.windows(2).enumerate().all(|(index, pair)| {
                spelling_separator(&text[pair[0].0.end..pair[1].0.start])
                    == expected.separators.get(index).copied()
            });
            let range = window.first()?.0.start..window.last()?.0.end;
            (tokens_match && separators_match && !is_protected_token(text, range.start, range.end))
                .then_some(range)
        })
        .collect()
}

fn spelling_pattern(value: &str) -> Option<SpellingPattern> {
    let value = value.trim();
    let ranges = spelling_token_ranges(value).collect::<Vec<_>>();
    if ranges.is_empty()
        || !value[..ranges[0].start].trim().is_empty()
        || !value[ranges.last()?.end..].trim().is_empty()
    {
        return None;
    }
    let tokens = ranges
        .iter()
        .map(|range| standard_spelling_key(&value[range.clone()]))
        .collect::<Option<Vec<_>>>()?;
    let separators = ranges
        .windows(2)
        .map(|pair| spelling_separator(&value[pair[0].end..pair[1].start]))
        .collect::<Option<Vec<_>>>()?;
    Some(SpellingPattern { tokens, separators })
}

fn spelling_separator(value: &str) -> Option<SpellingSeparator> {
    if !value.is_empty() && value.chars().all(char::is_whitespace) {
        Some(SpellingSeparator::Space)
    } else if value.trim() == "-"
        && value
            .chars()
            .all(|character| character == '-' || character.is_whitespace())
    {
        Some(SpellingSeparator::Hyphen)
    } else {
        None
    }
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
    fn recalls_only_canonical_terms_across_language_tags() {
        let entries = vec![
            entry("en", "OpenAI"),
            entry("zh-Hans", "SQLite"),
            entry("en", "GitHub"),
        ];

        assert_eq!(
            vec![
                RefinementTerm {
                    canonical: "OpenAI".to_owned(),
                },
                RefinementTerm {
                    canonical: "SQLite".to_owned(),
                },
            ],
            select_relevant_terms(entries, "请用 openai 和 sqlite", "zh-Hans")
        );
    }

    #[test]
    fn noncanonical_spellings_are_not_replaced() {
        let terms = vec![
            RefinementTerm {
                canonical: "OpenAI".to_owned(),
            },
            RefinementTerm {
                canonical: "GitHub".to_owned(),
            },
        ];

        assert_eq!(
            "使用 open ai 和 Git Hub，保留 myopenai 与 https://git hub.com",
            normalize_standard_spellings(
                "使用 open ai 和 Git Hub，保留 myopenai 与 https://git hub.com",
                &terms,
            )
        );
    }

    #[test]
    fn noncanonical_spellings_do_not_recall_a_term() {
        let entries = vec![entry("en", "OpenAI")];

        assert!(select_relevant_terms(entries.clone(), "使用 open ai", "zh-Hans").is_empty());
        assert!(select_relevant_terms(entries, "使用 open-ai", "zh-Hans").is_empty());
    }

    fn entry(language: &str, canonical: &str) -> DictionaryEntry {
        DictionaryEntry {
            id: format!("{language}-{canonical}"),
            canonical: canonical.to_owned(),
            language: language.to_owned(),
            origin: DictionaryOrigin::Manual,
            created_at_ms: 1,
            updated_at_ms: 1,
        }
    }
}
