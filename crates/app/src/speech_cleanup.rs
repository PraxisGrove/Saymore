//! High-confidence, source-preserving cleanup before LLM refinement.

const STANDALONE_FILLERS: [&str; 6] = ["嗯", "呃", "啊", "那个", "就是说", "you know"];
const REPEATED_CJK_STUTTERS: [char; 4] = ['我', '你', '他', '这'];

/// Removes only high-confidence speech noise without inventing replacement text.
///
/// The cleanup intentionally leaves self-corrections, sentence reordering,
/// terminology, numbers, and ambiguous fillers for the refinement policy.
pub fn clean_speech(transcript: &str) -> String {
    let normalized = normalize_whitespace(transcript);
    let without_fillers = remove_standalone_fillers(&normalized);
    let without_stutters = remove_repeated_cjk_stutters(&without_fillers);
    remove_repeated_ascii_words(&without_stutters)
}

pub(super) fn normalize_whitespace(transcript: &str) -> String {
    transcript
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

fn remove_standalone_fillers(text: &str) -> String {
    let mut output = text.to_owned();
    for filler in STANDALONE_FILLERS {
        for delimiter in ["，", ",", "、", "。", "！", "!", "？", "?", "；", ";", "\n"] {
            output = output.replace(&format!("{filler}{delimiter}"), delimiter);
        }
        if output == filler {
            output.clear();
        }
    }
    output
        .trim_start_matches(['，', ',', '、', '。', '！', '!', '？', '?', '；', ';'])
        .trim()
        .to_owned()
}

fn remove_repeated_cjk_stutters(text: &str) -> String {
    let characters = text.chars().collect::<Vec<_>>();
    let mut output = String::with_capacity(text.len());
    let mut index = 0;
    while index < characters.len() {
        if index + 1 < characters.len()
            && characters[index] == characters[index + 1]
            && REPEATED_CJK_STUTTERS.contains(&characters[index])
        {
            output.push(characters[index]);
            index += 2;
        } else {
            output.push(characters[index]);
            index += 1;
        }
    }
    output
}

fn remove_repeated_ascii_words(text: &str) -> String {
    let mut output = Vec::new();
    let mut previous: Option<&str> = None;
    for token in text.split_inclusive(|character: char| character.is_whitespace()) {
        let word = token.trim();
        let is_word = !word.is_empty()
            && word
                .chars()
                .all(|character| character.is_ascii_alphabetic());
        if is_word && previous.is_some_and(|value| value.eq_ignore_ascii_case(word)) {
            continue;
        }
        output.push(token);
        if is_word {
            previous = Some(word);
        } else if !word.is_empty() {
            previous = None;
        }
    }
    output.concat().trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::clean_speech;

    #[test]
    fn removes_only_standalone_fillers() {
        assert_eq!(
            "我们看一下那个按钮。",
            clean_speech("嗯，我们看一下那个按钮。")
        );
        assert_eq!("我们看一下那个按钮。", clean_speech("我们看一下那个按钮。"));
        assert_eq!("那个按钮需要调整。", clean_speech("那个按钮需要调整。"));
    }

    #[test]
    fn removes_high_confidence_stutters() {
        assert_eq!(
            "我觉得这个方案可以。",
            clean_speech("我我觉得这个方案可以。")
        );
        assert_eq!("I think this works.", clean_speech("I think this works."));
        assert_eq!(
            "I think this works.",
            clean_speech("I think think this works.")
        );
    }

    #[test]
    fn leaves_repairs_numbers_and_ambiguous_fillers_unchanged() {
        let source = "我准备下午三点，不对，下午五点左右去公园。那个按钮需要调整。";
        assert_eq!(source, clean_speech(source));
    }

    #[test]
    fn normalizes_whitespace_without_rewriting_content() {
        assert_eq!(
            "hello world\nnext line",
            clean_speech("  hello   world  \n  next line  ")
        );
    }
}
