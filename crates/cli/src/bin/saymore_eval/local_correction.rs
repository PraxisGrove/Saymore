use std::time::Instant;

use serde::Serialize;
use template_app::{RefinementTerm, normalize_standard_spellings};

use super::{
    metrics::text_metrics,
    rules::{EvaluationRule, RuleResult, evaluate_rules, pass_rate},
};

#[derive(Debug, Serialize)]
pub struct LocalCorrectionResult {
    pub text: String,
    pub matched_terms: Vec<String>,
    pub duration_us: u128,
    pub exact_match: bool,
    pub surface_character_error_rate: f64,
    pub content_character_error_rate: f64,
    pub punctuation_score: f64,
    pub structure_match: bool,
    pub rule_results: Vec<RuleResult>,
    pub rule_pass_rate: Option<f64>,
}

pub fn correct(
    transcript: &str,
    expected: &str,
    terms: &[RefinementTerm],
    rules: &[EvaluationRule],
) -> LocalCorrectionResult {
    let started = Instant::now();
    let text = normalize_standard_spellings(transcript, terms);
    let duration_us = started.elapsed().as_micros();
    let metrics = text_metrics(expected, &text);
    let rule_results = evaluate_rules(rules, &text);
    LocalCorrectionResult {
        exact_match: text.trim() == expected.trim(),
        matched_terms: terms.iter().map(|term| term.canonical.clone()).collect(),
        duration_us,
        surface_character_error_rate: metrics.surface_character_error_rate,
        content_character_error_rate: metrics.content_character_error_rate,
        punctuation_score: metrics.punctuation_score,
        structure_match: metrics.structure_match,
        rule_pass_rate: pass_rate(&rule_results),
        rule_results,
        text,
    }
}
