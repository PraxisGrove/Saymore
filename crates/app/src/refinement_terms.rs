use std::cmp::Reverse;

use crate::final_text_processing::RefinementTerm;

pub(crate) fn normalize_confirmed_terms(text: &str, relevant_terms: &[RefinementTerm]) -> String {
    let mut aliases = relevant_terms
        .iter()
        .flat_map(|term| {
            term.recognized_as
                .iter()
                .map(move |alias| (alias.as_str(), term.canonical.as_str()))
        })
        .filter(|(alias, canonical)| !alias.is_empty() && !canonical.is_empty())
        .collect::<Vec<_>>();
    aliases.sort_by_key(|alias| Reverse(alias.0.len()));

    let mut normalized = String::with_capacity(text.len());
    let mut index = 0;
    while index < text.len() {
        let replacement = aliases
            .iter()
            .find(|(alias, _)| alias_matches_at(text, index, alias));
        if let Some((alias, canonical)) = replacement {
            normalized.push_str(canonical);
            index += alias.len();
        } else if let Some(character) = text[index..].chars().next() {
            normalized.push(character);
            index += character.len_utf8();
        } else {
            break;
        }
    }
    normalized
}

fn alias_matches_at(text: &str, start: usize, alias: &str) -> bool {
    if !text[start..].starts_with(alias) {
        return false;
    }
    let end = start + alias.len();
    let starts_inside_ascii_word = alias
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_alphanumeric())
        && text[..start]
            .chars()
            .next_back()
            .is_some_and(|character| character.is_ascii_alphanumeric());
    let ends_inside_ascii_word = alias
        .chars()
        .next_back()
        .is_some_and(|character| character.is_ascii_alphanumeric())
        && text[end..]
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphanumeric());
    !starts_inside_ascii_word && !ends_inside_ascii_word
}
