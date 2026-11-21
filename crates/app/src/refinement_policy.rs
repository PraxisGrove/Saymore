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
    let characters = text.chars().collect::<Vec<_>>();
    let mut fragments = Vec::new();
    let mut index = 0;
    while index < characters.len() {
        if !is_numeric_character(characters[index]) {
            index += 1;
            continue;
        }
        let start = index;
        index += 1;
        while index < characters.len()
            && (is_numeric_character(characters[index]) || matches!(characters[index], '.' | ':'))
        {
            index += 1;
        }
        let core = characters[start..index].iter().collect::<String>();
        let unit_start = index;
        while index < characters.len() && characters[index].is_whitespace() {
            index += 1;
        }
        let unit_value_start = index;
        while index < characters.len() && is_numeric_unit(characters[index]) {
            index += 1;
        }
        let unit = characters[unit_value_start..index]
            .iter()
            .collect::<String>();
        if unit.is_empty() {
            index = unit_start;
        }
        let normalized = normalize_numeric_fragment(&core);
        if !normalized.is_empty() {
            fragments.push(format!("{normalized}{unit}"));
        }
    }
    fragments
}

fn is_numeric_character(character: char) -> bool {
    character.is_ascii_digit()
        || matches!(
            character,
            '零' | '〇'
                | '一'
                | '二'
                | '两'
                | '三'
                | '四'
                | '五'
                | '六'
                | '七'
                | '八'
                | '九'
                | '十'
                | '百'
                | '千'
                | '万'
                | '亿'
        )
}

fn is_numeric_unit(character: char) -> bool {
    matches!(
        character,
        '年' | '月'
            | '日'
            | '号'
            | '点'
            | '时'
            | '分'
            | '秒'
            | '个'
            | '元'
            | '块'
            | '人'
            | '张'
            | '台'
            | '件'
            | '天'
            | '周'
            | '米'
            | '半'
            | '%'
    )
}

fn normalize_numeric_fragment(fragment: &str) -> String {
    let trimmed = fragment.trim_end_matches(['.', ':']);
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed
        .chars()
        .all(|character| character.is_ascii_digit() || matches!(character, '.' | ':'))
    {
        return trimmed.to_owned();
    }
    normalize_chinese_number(trimmed).unwrap_or_else(|| {
        trimmed
            .chars()
            .map(|character| match character {
                '零' | '〇' => '0',
                '一' => '1',
                '二' | '两' => '2',
                '三' => '3',
                '四' => '4',
                '五' => '5',
                '六' => '6',
                '七' => '7',
                '八' => '8',
                '九' => '9',
                '十' => '1',
                '百' => '0',
                '千' => '0',
                '万' => '0',
                '亿' => '0',
                other => other,
            })
            .collect()
    })
}

fn normalize_chinese_number(value: &str) -> Option<String> {
    if value.chars().all(|character| {
        matches!(
            character,
            '零' | '〇' | '一' | '二' | '两' | '三' | '四' | '五' | '六' | '七' | '八' | '九'
        )
    }) {
        return Some(
            value
                .chars()
                .map(|character| match character {
                    '零' | '〇' => '0',
                    '一' => '1',
                    '二' | '两' => '2',
                    '三' => '3',
                    '四' => '4',
                    '五' => '5',
                    '六' => '6',
                    '七' => '7',
                    '八' => '8',
                    '九' => '9',
                    _ => '0',
                })
                .collect(),
        );
    }
    let mut total = 0_u64;
    let mut section = 0_u64;
    let mut number = 0_u64;
    for character in value.chars() {
        let digit = match character {
            '零' | '〇' => Some(0),
            '一' => Some(1),
            '二' | '两' => Some(2),
            '三' => Some(3),
            '四' => Some(4),
            '五' => Some(5),
            '六' => Some(6),
            '七' => Some(7),
            '八' => Some(8),
            '九' => Some(9),
            _ => None,
        };
        if let Some(digit) = digit {
            number = number.checked_mul(10)?.checked_add(digit)?;
            continue;
        }
        let multiplier = match character {
            '十' => 10,
            '百' => 100,
            '千' => 1_000,
            '万' => 10_000,
            '亿' => 100_000_000,
            _ => return None,
        };
        if multiplier >= 10_000 {
            section = section.checked_add(number)?.checked_mul(multiplier)?;
            total = total.checked_add(section)?;
            section = 0;
        } else {
            let value = if number == 0 { 1 } else { number };
            section = section.checked_add(value.checked_mul(multiplier)?)?;
        }
        number = 0;
    }
    total
        .checked_add(section)?
        .checked_add(number)
        .map(|value| value.to_string())
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
            .map(normalize_technical_token)
            .filter(|token| {
                is_technical_token(token) || (command_context && is_ascii_command_token(token))
            }),
    )
}

fn immutable_technical_fragments(text: &str) -> BTreeMap<String, usize> {
    fragment_counts(
        text.split_whitespace()
            .map(trim_token_boundaries)
            .map(normalize_technical_token)
            .filter(|token| is_technical_token(token) && !has_internal_uppercase(token)),
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

fn normalize_technical_token(token: &str) -> String {
    token
        .chars()
        .map(|character| match character {
            '\u{ff01}'..='\u{ff5e}' => match char::from_u32(character as u32 - 0xfee0) {
                Some(ascii) => ascii,
                None => character,
            },
            '\u{3002}' => '.',
            other => other,
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use super::validate_refinement;
    use crate::final_text_processing::{RefinementOutputRejectionReason, RefinementTerm};

    fn terms() -> Vec<RefinementTerm> {
        Vec::new()
    }

    #[test]
    fn accepts_numeric_format_normalization() {
        let result = validate_refinement("会议安排在下午三点。", "会议安排在下午 3 点。", &terms());
        assert_eq!(Ok(()), result);
    }

    #[test]
    fn rejects_numeric_value_changes() {
        let result =
            validate_refinement("会议安排在下午 3 点。", "会议安排在下午 4 点。", &terms());
        assert_eq!(
            Err(RefinementOutputRejectionReason::NumericFactsChanged),
            result
        );
    }

    #[test]
    fn accepts_chinese_date_normalization() {
        let result = validate_refinement(
            "发布日期是二零二六年十一月。",
            "发布日期是 2026 年 11 月。",
            &terms(),
        );
        assert_eq!(Ok(()), result);
    }

    #[test]
    fn normalizes_full_width_technical_tokens() {
        assert_eq!(
            "--lib",
            super::normalize_technical_token("--\u{ff4c}\u{ff49}\u{ff42}"),
        );
    }

    #[test]
    fn rejects_changed_paths() {
        let result = validate_refinement(
            "请打开 /Users/yaco/Projects/Saymore。",
            "请打开 /Users/yaco/Projects/Other。",
            &terms(),
        );
        assert_eq!(
            Err(RefinementOutputRejectionReason::ProtectedFragmentChanged),
            result
        );
    }

    #[test]
    fn preserves_numeric_units() {
        let result = validate_refinement("需要 3 个人。", "需要 3 元。", &terms());
        assert_eq!(
            Err(RefinementOutputRejectionReason::NumericFactsChanged),
            result
        );
    }
}
