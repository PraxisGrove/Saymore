use std::collections::BTreeSet;

use template_app::SpeechRecognitionHints;

// Bound the session vocabulary to keep prompts concise and predictable.
const MAX_HOTWORD_BYTES: usize = 128;

pub(super) fn from_hints(hints: &SpeechRecognitionHints) -> String {
    let mut seen = BTreeSet::new();
    let mut hotwords = String::new();
    for term in hints.terms().iter().map(|term| term.trim()) {
        // Only canonical vocabulary enters the model prompt; omit control characters.
        if term.is_empty()
            || term.chars().any(char::is_control)
            || !seen.insert(term.to_lowercase())
        {
            continue;
        }
        let separator = if hotwords.is_empty() { "" } else { " " };
        if hotwords.len() + separator.len() + term.len() > MAX_HOTWORD_BYTES {
            continue;
        }
        hotwords.push_str(separator);
        hotwords.push_str(term);
    }
    hotwords
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_canonical_terms_and_drops_duplicates_and_controls() {
        let hints = SpeechRecognitionHints::from_terms(
            [
                " Saymore ",
                "声墨",
                "saymore",
                "",
                "bad\0term",
                "bad\nterm",
                "Slint",
            ]
            .map(str::to_owned)
            .to_vec(),
        );
        assert_eq!(concat!("Saymore ", "声墨", " Slint"), from_hints(&hints));
        assert_eq!("", from_hints(&SpeechRecognitionHints::default()));
    }

    #[test]
    fn bounds_utf8_bytes_without_cutting_terms_and_keeps_later_short_terms() {
        let hints = SpeechRecognitionHints::from_terms(vec![
            "长".repeat(MAX_HOTWORD_BYTES),
            "声".repeat(40),
            "too-long-to-fit".to_owned(),
            "Slint".to_owned(),
        ]);
        assert_eq!(format!("{} Slint", "声".repeat(40)), from_hints(&hints));
    }
}
