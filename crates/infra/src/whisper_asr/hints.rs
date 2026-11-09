use std::collections::HashSet;

// Byte-level BPE yields at most one token per byte. This conservative bound
// keeps the entire prompt below Whisper's 224-token initial-context window.
const MAX_PROMPT_BYTES: usize = 192;

pub(super) fn vocabulary_prompt(terms: &[String]) -> String {
    let mut seen = HashSet::new();
    let mut prompt = String::new();
    for term in terms {
        let term = term.trim();
        if term.is_empty() || term.chars().any(char::is_control) {
            continue;
        }
        let separator = if prompt.is_empty() { "" } else { ", " };
        if prompt.len() + separator.len() + term.len() > MAX_PROMPT_BYTES
            || !seen.insert(term.to_lowercase())
        {
            continue;
        }
        prompt.push_str(separator);
        prompt.push_str(term);
    }
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_canonical_spelling_and_omits_duplicates_and_controls() {
        let terms = [" Saymore ", "saymore", "思源黑体", "", "a\0b", "a\nb"].map(str::to_owned);
        assert_eq!("Saymore, 思源黑体", vocabulary_prompt(&terms));
    }

    #[test]
    fn keeps_whole_utf8_terms_and_can_use_short_terms_after_an_oversized_one() {
        let first = "词".repeat(60);
        let terms = vec![first.clone(), "过长的词条".repeat(20), "Slint".to_owned()];
        assert_eq!(format!("{first}, Slint"), vocabulary_prompt(&terms));
    }
}
