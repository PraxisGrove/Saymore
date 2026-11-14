use super::protocol;
use std::collections::HashSet;
use template_app::SpeechRecognitionError;
use tokenizers::Tokenizer;

pub(super) const AUDIO_TOKEN: i64 = 151671;
const INSTRUCTION: &str = "请将音频转写为文本，每一段需以起始时间戳和说话人编号（[S01]、[S02]、[S03]…）开头，正文为对应的语音内容，并在段末标注结束时间戳，以清晰标明该段语音范围。";

pub(super) fn clean_hints(terms: &[String]) -> String {
    let mut seen = HashSet::new();
    let mut result = String::new();
    for term in terms {
        let term = term.trim();
        if term.is_empty()
            || term.chars().any(|c| c.is_control() || "[]<>|".contains(c))
            || !seen.insert(term.to_lowercase())
        {
            continue;
        }
        let separator = if result.is_empty() { "" } else { ", " };
        if result.len() + separator.len() + term.len() > 256 {
            continue;
        }
        result.push_str(separator);
        result.push_str(term);
        if result.split(", ").count() >= 64 {
            break;
        }
    }
    result
}

pub(super) fn build(
    tokenizer: &Tokenizer,
    audio_frames: usize,
    hints: &str,
) -> Result<Vec<i64>, SpeechRecognitionError> {
    let mut tokens = encode(
        tokenizer,
        "<|im_start|>system\nYou are a helpful assistant.<|im_end|>\n<|im_start|>user\n<|audio_start|>",
    )?;
    for index in 0..audio_frames {
        tokens.push(AUDIO_TOKEN);
        if (index + 1).is_multiple_of(62) && (index + 1) / 62 <= audio_frames * 2 / 125 {
            tokens.extend(encode(tokenizer, &((index + 1) / 62 * 5).to_string())?);
        }
    }
    let hint = if hints.is_empty() {
        String::new()
    } else {
        format!("热词提示：{hints}")
    };
    tokens.extend(encode(
        tokenizer,
        &format!("<|audio_end|>\n{INSTRUCTION}{hint}<|im_end|>\n<|im_start|>assistant\n"),
    )?);
    Ok(tokens)
}

fn encode(tokenizer: &Tokenizer, text: &str) -> Result<Vec<i64>, SpeechRecognitionError> {
    tokenizer
        .encode(text, false)
        .map(|encoding| encoding.get_ids().iter().map(|id| i64::from(*id)).collect())
        .map_err(|_| protocol("could not encode the prompt"))
}
