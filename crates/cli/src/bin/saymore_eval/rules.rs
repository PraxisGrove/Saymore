use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EvaluationRule {
    Contains { label: String, value: String },
    NotContains { label: String, value: String },
    StartsWith { label: String, value: String },
    EndsWith { label: String, value: String },
    OrderedContains { label: String, values: Vec<String> },
    ParagraphCount { label: String, value: usize },
    NumberedListCount { label: String, value: usize },
    EndsWithoutPunctuation { label: String },
    Exact { label: String, value: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct RuleResult {
    pub label: String,
    pub passed: bool,
}

pub fn evaluate_rules(rules: &[EvaluationRule], text: &str) -> Vec<RuleResult> {
    rules
        .iter()
        .map(|rule| RuleResult {
            label: rule.label().to_owned(),
            passed: rule.evaluate(text),
        })
        .collect()
}

pub fn pass_rate(results: &[RuleResult]) -> Option<f64> {
    (!results.is_empty()).then(|| {
        results.iter().filter(|result| result.passed).count() as f64 / results.len() as f64
    })
}

impl EvaluationRule {
    fn label(&self) -> &str {
        match self {
            Self::Contains { label, .. }
            | Self::NotContains { label, .. }
            | Self::StartsWith { label, .. }
            | Self::EndsWith { label, .. }
            | Self::OrderedContains { label, .. }
            | Self::ParagraphCount { label, .. }
            | Self::NumberedListCount { label, .. }
            | Self::EndsWithoutPunctuation { label }
            | Self::Exact { label, .. } => label,
        }
    }

    fn evaluate(&self, text: &str) -> bool {
        let text = text.trim();
        match self {
            Self::Contains { value, .. } => text.contains(value),
            Self::NotContains { value, .. } => !text.contains(value),
            Self::StartsWith { value, .. } => text.starts_with(value),
            Self::EndsWith { value, .. } => text.ends_with(value),
            Self::OrderedContains { values, .. } => ordered_contains(text, values),
            Self::ParagraphCount { value, .. } => {
                (!text.is_empty() && text.split("\n\n").count() == *value)
                    || (text.is_empty() && *value == 0)
            }
            Self::NumberedListCount { value, .. } => {
                text.lines().filter(|line| is_numbered_item(line)).count() == *value
            }
            Self::EndsWithoutPunctuation { .. } => {
                text.chars().next_back().is_some_and(|character| {
                    !matches!(
                        character,
                        '。' | '！' | '？' | '!' | '?' | '；' | ';' | '，' | ','
                    )
                })
            }
            Self::Exact { value, .. } => text == value.trim(),
        }
    }
}

fn ordered_contains(text: &str, values: &[String]) -> bool {
    let mut remaining = text;
    for value in values {
        let Some(index) = remaining.find(value) else {
            return false;
        };
        remaining = &remaining[index + value.len()..];
    }
    true
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
    fn evaluates_order_structure_and_unfinished_endings() {
        let rules = vec![
            EvaluationRule::OrderedContains {
                label: "顺序".to_owned(),
                values: vec!["如果".to_owned(), "我们".to_owned()],
            },
            EvaluationRule::EndsWithoutPunctuation {
                label: "未完成".to_owned(),
            },
        ];
        let results = evaluate_rules(&rules, "如果测试失败，我们还需要");

        assert!(results.iter().all(|result| result.passed));
        assert_eq!(Some(1.0), pass_rate(&results));
    }
}
