use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use template_app::SpeechRecognitionError;
use unicode_normalization::UnicodeNormalization;

use super::{protocol, read_text};

pub(super) const VOCAB_SIZE: usize = 8_404;
pub(super) const WORD_WIDTH: usize = 10;
pub(super) const EMBEDDING_DIM: usize = 512;
const MAX_TERMS: usize = 64;

pub(super) struct Vocabulary {
    tokens: Vec<String>,
    ids: HashMap<String, i32>,
    segmentation: HashMap<String, Vec<String>>,
}

/// Padded token rows and their valid lengths, including the final ID-1 sentinel.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Hotwords {
    pub matrix: Vec<i32>,
    pub lengths: Vec<usize>,
}

impl Vocabulary {
    pub fn load(tokens: &Path, segmentation: &Path) -> Result<Self, SpeechRecognitionError> {
        let tokens: Vec<String> = serde_json::from_str(&read_text(tokens)?)
            .map_err(|_| protocol("token vocabulary is invalid"))?;
        if tokens.len() != VOCAB_SIZE || tokens.iter().any(String::is_empty) {
            return Err(protocol("requires the 8404-token vocabulary"));
        }
        let ids = tokens
            .iter()
            .enumerate()
            .map(|(id, token)| (token.clone(), id as i32))
            .collect();
        let segmentation = read_text(segmentation)?
            .lines()
            .filter_map(|line| {
                let mut fields = line.split_whitespace();
                let word = fields.next()?.to_owned();
                let pieces: Vec<String> = fields.map(ToOwned::to_owned).collect();
                (!pieces.is_empty()).then_some((word, pieces))
            })
            .collect();
        Ok(Self {
            tokens,
            ids,
            segmentation,
        })
    }

    pub fn hotwords(&self, terms: &[String]) -> Hotwords {
        let mut matrix = Vec::new();
        let mut lengths = Vec::new();
        let mut seen = HashSet::new();
        // Bound work even if a caller supplies a mostly unsupported dictionary.
        for term in terms.iter().take(512) {
            if lengths.len() == MAX_TERMS {
                break;
            }
            if term.len() > 512 || term.chars().any(char::is_control) {
                continue;
            }
            let normalized: String = term.trim().nfc().collect::<String>().to_lowercase();
            let Some(ids) = self.term_ids(&normalized) else {
                continue;
            };
            if ids.is_empty() || ids.len() > WORD_WIDTH || !seen.insert(ids.clone()) {
                continue;
            }
            lengths.push(ids.len());
            let mut row = [0; WORD_WIDTH];
            row[..ids.len()].copy_from_slice(&ids);
            matrix.extend(row);
        }
        let mut sentinel = [0; WORD_WIDTH];
        sentinel[0] = 1;
        matrix.extend(sentinel);
        lengths.push(1);
        Hotwords { matrix, lengths }
    }

    fn term_ids(&self, term: &str) -> Option<Vec<i32>> {
        let mut ids = Vec::new();
        for word in term.split_whitespace() {
            if word.chars().all(is_chinese) {
                for character in word.chars() {
                    ids.push(*self.ids.get(&character.to_string())?);
                }
            } else {
                for piece in self.segmentation.get(word)? {
                    let id = *self.ids.get(piece)?;
                    // Padding, sentinel, unknown and NO_BIAS are not vocabulary hints.
                    if id < 3 || id == 8_377 {
                        return None;
                    }
                    ids.push(id);
                }
            }
            if ids.len() > WORD_WIDTH {
                return None;
            }
        }
        Some(ids)
    }

    pub fn text(&self, ids: &[usize]) -> Result<String, SpeechRecognitionError> {
        let mut text = String::new();
        let mut pending = String::new();
        let mut previous_english_length = 0;
        for &id in ids {
            let token = self
                .tokens
                .get(id)
                .ok_or_else(|| protocol("decoded an invalid token ID"))?;
            if token.starts_with('<') && token.ends_with('>') {
                continue;
            }
            if let Some(piece) = token.strip_suffix("@@") {
                pending.push_str(piece);
                continue;
            }
            if token.chars().any(is_chinese) && !pending.is_empty() {
                append_word(&mut text, &pending, &mut previous_english_length);
                pending.clear();
            }
            pending.push_str(token);
            append_word(&mut text, &pending, &mut previous_english_length);
            pending.clear();
        }
        if !pending.is_empty() {
            append_word(&mut text, &pending, &mut previous_english_length);
        }
        Ok(text)
    }
}

fn append_word(text: &mut String, word: &str, previous_english_length: &mut usize) {
    let english = word
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '\'');
    if english && *previous_english_length > 0 && (*previous_english_length > 1 || word.len() > 1) {
        text.push(' ');
    }
    text.push_str(word);
    *previous_english_length = if english { word.len() } else { 0 };
}

fn is_chinese(character: char) -> bool {
    matches!(character as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x3134F)
}

#[cfg(test)]
#[allow(clippy::panic_in_result_fn)]
mod tests {
    use super::*;

    pub(super) fn vocabulary() -> Vocabulary {
        let tokens: Vec<String> = [
            "<blank>", "<s>", "</s>", "魔", "搭", "hell@@", "o", "world", "社", "区",
        ]
        .into_iter()
        .map(ToOwned::to_owned)
        .collect();
        Vocabulary {
            ids: tokens
                .iter()
                .enumerate()
                .map(|(id, token)| (token.clone(), id as i32))
                .collect(),
            tokens,
            segmentation: HashMap::from([
                (
                    "hello".to_owned(),
                    vec!["hell@@".to_owned(), "o".to_owned()],
                ),
                ("world".to_owned(), vec!["world".to_owned()]),
            ]),
        }
    }

    #[test]
    fn canonical_hints_are_deduplicated_and_unknown_terms_do_not_poison_the_batch() {
        let vocab = vocabulary();
        let terms = [
            " 魔搭 ",
            "魔搭",
            "Hello world",
            "unknown",
            "魔搭魔搭魔搭魔搭魔搭魔",
            "\n魔搭",
        ]
        .into_iter()
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
        let hotwords = vocab.hotwords(&terms);
        assert_eq!(vec![2, 3, 1], hotwords.lengths);
        assert_eq!(&[3, 4, 0, 0, 0, 0, 0, 0, 0, 0], &hotwords.matrix[..10]);
        assert_eq!(&[5, 6, 7, 0, 0, 0, 0, 0, 0, 0], &hotwords.matrix[10..20]);
        assert_eq!(&[1, 0, 0, 0, 0, 0, 0, 0, 0, 0], &hotwords.matrix[20..]);
    }

    #[test]
    fn large_dictionaries_preserve_the_limit_and_final_sentinel() {
        let terms = (0..80)
            .map(|word| {
                (0..7)
                    .map(|bit| if word & (1 << bit) == 0 { '魔' } else { '搭' })
                    .collect()
            })
            .collect::<Vec<String>>();
        let hotwords = vocabulary().hotwords(&terms);
        assert_eq!(vec![7; 64], hotwords.lengths[..64]);
        assert_eq!(65, hotwords.lengths.len());
        assert_eq!(&[1, 0, 0, 0, 0, 0, 0, 0, 0, 0], &hotwords.matrix[640..]);
    }

    #[test]
    fn mixed_chinese_english_subwords_and_special_tokens_decode_without_replacement()
    -> Result<(), SpeechRecognitionError> {
        assert_eq!(
            concat!("魔搭", "hello world", "社区"),
            vocabulary().text(&[0, 1, 3, 4, 5, 6, 7, 8, 9, 2])?
        );
        Ok(())
    }
}
