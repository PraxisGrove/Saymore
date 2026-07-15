use unicode_normalization::UnicodeNormalization;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextMetrics {
    pub surface_character_error_rate: f64,
    pub content_character_error_rate: f64,
    pub punctuation_score: f64,
    pub structure_match: bool,
}

pub fn text_metrics(expected: &str, actual: &str) -> TextMetrics {
    TextMetrics {
        surface_character_error_rate: error_rate(
            &surface_characters(expected),
            &surface_characters(actual),
        ),
        content_character_error_rate: error_rate(
            &content_characters(expected),
            &content_characters(actual),
        ),
        punctuation_score: sequence_similarity(
            &punctuation_characters(expected),
            &punctuation_characters(actual),
        ),
        structure_match: structure_signature(expected) == structure_signature(actual),
    }
}

fn surface_characters(value: &str) -> Vec<char> {
    value
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

fn content_characters(value: &str) -> Vec<char> {
    value
        .nfkc()
        .flat_map(char::to_lowercase)
        .filter(|character| character.is_alphanumeric())
        .collect()
}

fn punctuation_characters(value: &str) -> Vec<char> {
    value
        .chars()
        .filter(|character| !character.is_alphanumeric() && !character.is_whitespace())
        .collect()
}

fn error_rate(expected: &[char], actual: &[char]) -> f64 {
    if expected.is_empty() {
        return if actual.is_empty() { 0.0 } else { 1.0 };
    }
    levenshtein(expected, actual) as f64 / expected.len() as f64
}

fn sequence_similarity(expected: &[char], actual: &[char]) -> f64 {
    let maximum = expected.len().max(actual.len());
    if maximum == 0 {
        return 1.0;
    }
    1.0 - levenshtein(expected, actual) as f64 / maximum as f64
}

fn levenshtein(left: &[char], right: &[char]) -> usize {
    let mut previous = (0..=right.len()).collect::<Vec<_>>();
    let mut current = vec![0; right.len() + 1];
    for (left_index, left_character) in left.iter().enumerate() {
        current[0] = left_index + 1;
        for (right_index, right_character) in right.iter().enumerate() {
            let substitution =
                previous[right_index] + usize::from(left_character != right_character);
            current[right_index + 1] = substitution
                .min(previous[right_index + 1] + 1)
                .min(current[right_index] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()]
}

#[derive(Debug, PartialEq, Eq)]
struct StructureSignature {
    paragraphs: usize,
    numbered_items: usize,
    bullet_items: usize,
}

fn structure_signature(value: &str) -> StructureSignature {
    let trimmed = value.trim();
    StructureSignature {
        paragraphs: if trimmed.is_empty() {
            0
        } else {
            trimmed.split("\n\n").count()
        },
        numbered_items: trimmed
            .lines()
            .filter(|line| is_numbered_item(line))
            .count(),
        bullet_items: trimmed
            .lines()
            .filter(|line| line.trim_start().starts_with("- "))
            .count(),
    }
}

fn is_numbered_item(line: &str) -> bool {
    let line = line.trim_start();
    let digit_count = line.chars().take_while(char::is_ascii_digit).count();
    digit_count > 0
        && line
            .get(digit_count..)
            .is_some_and(|suffix| suffix.starts_with(". "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_metric_ignores_punctuation_spacing_case_and_width() {
        let metrics = text_metrics("OpenAI，测试。", "ｏｐｅｎａｉ 测试");
        assert_eq!(0.0, metrics.content_character_error_rate);
        assert!(metrics.surface_character_error_rate > 0.0);
    }

    #[test]
    fn punctuation_and_structure_are_reported_separately() {
        let metrics = text_metrics("第一段。\n\n1. 第一步", "第一段，1. 第一步");
        assert!(metrics.punctuation_score < 1.0);
        assert!(!metrics.structure_match);
    }

    #[test]
    fn content_metric_counts_replacement_against_expected_length() {
        let metrics = text_metrics("测试", "测式");
        assert!((metrics.content_character_error_rate - 0.5).abs() < f64::EPSILON);
    }
}
