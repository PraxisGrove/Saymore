use std::collections::BTreeMap;

use crate::final_text_processing::{RefinementOutputRejectionReason, RefinementTerm};
use crate::refinement_terms::{normalize_spaced_standard_spellings, normalize_standard_spellings};

const OUTPUT_GROWTH_MULTIPLIER: usize = 2;
const OUTPUT_GROWTH_ALLOWANCE: usize = 32;
const WRAPPER_PREFIXES: [&str; 10] = [
    "润色结果",
    "精炼结果",
    "修改后",
    "输出结果",
    "以下是",
    "Here is",
    "Here's",
    "Refined text",
    "Polished text",
    "Output:",
];

pub(crate) fn validate_refinement(
    source: &str,
    candidate: &str,
    relevant_terms: &[RefinementTerm],
) -> Result<(), RefinementOutputRejectionReason> {
    let candidate = candidate.trim();
    if candidate.is_empty() {
        return Err(RefinementOutputRejectionReason::EmptyOutput);
    }
    if is_abnormally_large(source, candidate) {
        return Err(RefinementOutputRejectionReason::AbnormalGrowth);
    }
    if adds_non_refinement_wrapper(source, candidate) {
        return Err(RefinementOutputRejectionReason::NonRefinementWrapper);
    }
    if !technical_fragments_are_safe(source, candidate, relevant_terms) {
        return Err(RefinementOutputRejectionReason::ProtectedFragmentChanged);
    }
    if !numeric_fragments_are_safe(source, candidate, relevant_terms) {
        return Err(RefinementOutputRejectionReason::NumericFactsChanged);
    }
    Ok(())
}

fn technical_fragments_are_safe(
    source: &str,
    candidate: &str,
    relevant_terms: &[RefinementTerm],
) -> bool {
    if immutable_technical_fragments(source) != immutable_technical_fragments(candidate) {
        return false;
    }

    let source_fragments =
        technical_fragments(&normalize_spaced_standard_spellings(source, relevant_terms));
    let candidate_fragments =
        technical_fragments(&normalize_standard_spellings(candidate, relevant_terms));
    let trusted_fragments = technical_fragments(
        &relevant_terms
            .iter()
            .map(|term| term.canonical.as_str())
            .collect::<Vec<_>>()
            .join(" "),
    );

    source_fragments
        .iter()
        .all(|(fragment, count)| candidate_fragments.get(fragment).unwrap_or(&0) >= count)
        && candidate_fragments.iter().all(|(fragment, count)| {
            source_fragments.get(fragment).unwrap_or(&0) >= count
                || trusted_fragments.contains_key(fragment)
        })
}

fn is_abnormally_large(source: &str, candidate: &str) -> bool {
    let source_chars = source.chars().count();
    let maximum = source_chars
        .saturating_mul(OUTPUT_GROWTH_MULTIPLIER)
        .saturating_add(OUTPUT_GROWTH_ALLOWANCE);
    candidate.chars().count() > maximum
}

fn adds_non_refinement_wrapper(source: &str, candidate: &str) -> bool {
    let adds_known_prefix = WRAPPER_PREFIXES
        .iter()
        .any(|prefix| candidate.starts_with(prefix) && !source.starts_with(prefix));
    let adds_code_fence = candidate.contains("```") && !source.contains("```");
    let adds_heading = candidate.lines().any(|line| line.starts_with("# "))
        && !source.lines().any(|line| line.starts_with("# "));
    adds_known_prefix || adds_code_fence || adds_heading
}

fn numeric_fragments_are_safe(
    source: &str,
    candidate: &str,
    relevant_terms: &[RefinementTerm],
) -> bool {
    let source = normalize_standard_spellings(source, relevant_terms);
    let candidate = normalize_standard_spellings(candidate, relevant_terms);
    let source_facts = numeric_facts(&source);
    let candidate_facts = numeric_facts(&candidate);
    if source_facts.is_empty() {
        return true;
    }
    if source_facts == candidate_facts {
        return true;
    }
    if candidate_facts
        .iter()
        .any(|(fact, count)| source_facts.get(fact).unwrap_or(&0) < count)
    {
        return false;
    }
    let allowed_removals = explicitly_corrected_numeric_facts(&source);
    source_facts.iter().all(|(fact, source_count)| {
        let removed = source_count.saturating_sub(*candidate_facts.get(fact).unwrap_or(&0));
        removed <= *allowed_removals.get(fact).unwrap_or(&0)
    })
}

fn explicitly_corrected_numeric_facts(text: &str) -> BTreeMap<String, usize> {
    const MARKERS: [&str; 4] = ["不对", "改成", "应该是", "我是说"];
    let mut allowed = BTreeMap::new();
    for marker_start in MARKERS
        .iter()
        .flat_map(|marker| text.match_indices(marker).map(|(index, _)| index))
    {
        let prefix = text[..marker_start].trim_end_matches(|character| {
            matches!(
                character,
                '，' | ',' | '。' | '！' | '!' | '？' | '?' | '；' | ';' | '\n'
            )
        });
        let clause_start = prefix
            .char_indices()
            .rev()
            .find(|(_, character)| {
                matches!(
                    character,
                    '，' | ',' | '。' | '！' | '!' | '？' | '?' | '；' | ';' | '\n'
                )
            })
            .map_or(0, |(index, character)| index + character.len_utf8());
        for (fact, count) in numeric_facts(&prefix[clause_start..]) {
            *allowed.entry(fact).or_default() += count;
        }
    }
    allowed
}

fn numeric_facts(text: &str) -> BTreeMap<String, usize> {
    fragment_counts(numeric_fragments(text))
}

fn numeric_fragments(text: &str) -> Vec<String> {
    let mut fragments = Vec::new();
    let mut current = String::new();
    for character in text.chars() {
        if character.is_ascii_digit() || (!current.is_empty() && matches!(character, '.' | ':')) {
            current.push(character);
        } else if !current.is_empty() {
            fragments.push(current.trim_end_matches(['.', ':']).to_owned());
            current.clear();
        }
    }
    if !current.is_empty() {
        fragments.push(current.trim_end_matches(['.', ':']).to_owned());
    }
    fragments.retain(|fragment| !fragment.is_empty());
    fragments
}

fn technical_fragments(text: &str) -> BTreeMap<String, usize> {
    let command_context = text
        .split_whitespace()
        .map(trim_token_boundaries)
        .any(|token| is_command_flag(token) || is_known_command(token))
        || ["运行", "执行", "命令是", "run "]
            .iter()
            .any(|cue| text.contains(cue));
    fragment_counts(
        text.split_whitespace()
            .map(trim_token_boundaries)
            .filter(|token| {
                is_technical_token(token) || (command_context && is_ascii_command_token(token))
            })
            .map(str::to_owned),
    )
}

fn immutable_technical_fragments(text: &str) -> BTreeMap<String, usize> {
    fragment_counts(
        text.split_whitespace()
            .map(trim_token_boundaries)
            .filter(|token| is_technical_token(token) && !has_internal_uppercase(token))
            .map(str::to_owned),
    )
}

fn trim_token_boundaries(token: &str) -> &str {
    token.trim_matches(|character: char| {
        matches!(
            character,
            ',' | ';'
                | '!'
                | '?'
                | '，'
                | '。'
                | '；'
                | '！'
                | '？'
                | '、'
                | '('
                | ')'
                | '['
                | ']'
                | '{'
                | '}'
                | '<'
                | '>'
                | '“'
                | '”'
                | '"'
                | '\''
                | '`'
        )
    })
}

fn is_technical_token(token: &str) -> bool {
    token.contains("://")
        || token.starts_with("www.")
        || looks_like_email(token)
        || token.contains('/')
        || token.contains('\\')
        || token.contains("::")
        || token.contains("->")
        || token.contains('_')
        || is_command_flag(token)
        || looks_like_version(token)
        || has_internal_uppercase(token)
}

fn looks_like_email(token: &str) -> bool {
    token
        .split_once('@')
        .is_some_and(|(local, domain)| !local.is_empty() && domain.contains('.'))
}

fn is_command_flag(token: &str) -> bool {
    token.len() > 1 && token.starts_with('-')
}

fn is_known_command(token: &str) -> bool {
    const COMMANDS: [&str; 17] = [
        "cargo", "git", "npm", "pnpm", "yarn", "bun", "docker", "kubectl", "python", "python3",
        "go", "rustc", "make", "just", "curl", "ssh", "cd",
    ];
    COMMANDS.contains(&token)
}

fn looks_like_version(token: &str) -> bool {
    let value = token.strip_prefix(['v', 'V']).unwrap_or(token);
    value
        .chars()
        .next()
        .is_some_and(|first| first.is_ascii_digit())
        && value.split('.').count() >= 2
        && value.split('.').all(|part| !part.is_empty())
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '+')
        })
}

fn has_internal_uppercase(token: &str) -> bool {
    token
        .chars()
        .zip(token.chars().skip(1))
        .any(|(left, right)| left.is_ascii_lowercase() && right.is_ascii_uppercase())
}

fn is_ascii_command_token(token: &str) -> bool {
    token.chars().all(|character| {
        character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | ':' | '/')
    })
}

fn fragment_counts(
    fragments: impl IntoIterator<Item = impl Into<String>>,
) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for fragment in fragments {
        let count = counts.entry(fragment.into()).or_insert(0usize);
        *count = count.saturating_add(1);
    }
    counts
}
