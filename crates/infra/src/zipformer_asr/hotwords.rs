use std::collections::BTreeSet;

use template_app::SpeechRecognitionHints;

const MAX_TERMS: usize = 64;
const MAX_BYTES: usize = 1_024;
const MAX_TERM_CHARACTERS: usize = 64;

pub(super) fn supported_characters(tokens: &str) -> BTreeSet<char> {
    tokens
        .lines()
        .filter_map(|line| {
            let token = line.split_whitespace().next()?;
            let mut characters = token.chars();
            let character = characters.next()?;
            characters.next().is_none().then_some(character)
        })
        .collect()
}

pub(super) fn from_hints(hints: &SpeechRecognitionHints, vocabulary: &BTreeSet<char>) -> String {
    let mut seen = BTreeSet::new();
    let mut encoded = String::new();
    for term in hints
        .terms()
        .iter()
        .map(|term| term.trim().to_ascii_uppercase())
    {
        // This pinned model uses uppercase English tokens. Only send encodable
        // characters: native OOV diagnostics print terms. Grammar metacharacters
        // can introduce scores/thresholds, and controls can panic in CString.
        if term.is_empty()
            || term.chars().any(|ch| {
                ch.is_control()
                    || matches!(ch, '/' | ':' | '#' | '@')
                    || (ch != ' ' && !vocabulary.contains(&ch))
            })
            || term.chars().count() > MAX_TERM_CHARACTERS
            || seen.len() >= MAX_TERMS
            || seen.contains(&term.to_lowercase())
        {
            continue;
        }
        let separator = if encoded.is_empty() { "" } else { "/" };
        if encoded.len() + separator.len() + term.len() > MAX_BYTES {
            continue;
        }
        seen.insert(term.to_lowercase());
        encoded.push_str(separator);
        encoded.push_str(&term);
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    fn format(hints: &SpeechRecognitionHints) -> String {
        let vocabulary = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 声墨术语中长"
            .chars()
            .collect();
        from_hints(hints, &vocabulary)
    }

    #[test]
    fn snapshots_unique_phrases_without_native_grammar_injection() {
        let hints = SpeechRecognitionHints::from_terms(
            [
                " 声墨 ",
                "Saymore",
                "saymore",
                "hello world",
                "bad/term",
                "bad:100",
                "bad#1e999",
                "bad@term",
                "😀",
                "龘",
                "bad\0term",
                "bad\nterm",
                "",
            ]
            .map(str::to_owned)
            .to_vec(),
        );
        assert_eq!("声墨/SAYMORE/HELLO WORLD", format(&hints));
    }

    #[test]
    fn bounds_whole_utf8_terms_and_accepts_later_short_terms() {
        let mut terms = vec!["长".repeat(65)];
        terms.extend((0..64).map(|index| format!("术语{index}")));
        terms.push("overflow".to_owned());
        let result = format(&SpeechRecognitionHints::from_terms(terms));
        assert_eq!(64, result.split('/').count());
        assert!(!result.contains("OVERFLOW"));
        assert!(!result.contains('长'));
        let mut terms = (0..10)
            .map(|index| format!("{} {index}", "中".repeat(60)))
            .collect::<Vec<_>>();
        terms.push("short".to_owned());
        let result = format(&SpeechRecognitionHints::from_terms(terms));
        assert!(result.len() <= MAX_BYTES);
        assert!(result.ends_with("/SHORT"));
    }
}
