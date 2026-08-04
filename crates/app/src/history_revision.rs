use similar::{DiffTag, TextDiff};

const DIFF_CONTEXT_CHARS: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextRevisionEndReason {
    FocusLost,
    ControlReset,
    NextDictation,
    TimedOut,
    Cancelled,
}

impl TextRevisionEndReason {
    pub fn permits_dictionary_learning(self) -> bool {
        matches!(
            self,
            Self::FocusLost | Self::ControlReset | Self::NextDictation
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextRevisionEvent {
    Snapshot(String),
    Ended(TextRevisionEndReason),
}

pub type TextRevisionObserver = Box<dyn Fn(TextRevisionEvent) + Send + 'static>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinalTextRevision {
    pub original: String,
    pub final_text: String,
    pub end_reason: TextRevisionEndReason,
}

impl FinalTextRevision {
    pub fn permits_dictionary_learning(&self) -> bool {
        self.end_reason.permits_dictionary_learning()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinalTextRevisionState {
    original: String,
    latest: String,
    ended: bool,
}

impl FinalTextRevisionState {
    pub fn new(original: impl Into<String>) -> Self {
        let original = original.into();
        Self {
            latest: original.clone(),
            original,
            ended: false,
        }
    }

    pub fn handle(&mut self, event: TextRevisionEvent) -> Option<FinalTextRevision> {
        if self.ended {
            return None;
        }
        match event {
            TextRevisionEvent::Snapshot(text) => {
                if !text.trim().is_empty() {
                    self.latest = text;
                }
                None
            }
            TextRevisionEvent::Ended(reason) => {
                self.ended = true;
                (self.latest != self.original && reason != TextRevisionEndReason::Cancelled).then(
                    || FinalTextRevision {
                        original: self.original.clone(),
                        final_text: self.latest.clone(),
                        end_reason: reason,
                    },
                )
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextRevisionDiff {
    pub before: String,
    pub after: String,
    pub before_context: String,
    pub after_context: String,
    pub local_candidate: Option<String>,
}

pub fn text_revision_diffs(original: &str, final_text: &str) -> Vec<TextRevisionDiff> {
    let original_chars = original.chars().collect::<Vec<_>>();
    let final_chars = final_text.chars().collect::<Vec<_>>();
    let raw = TextDiff::from_chars(original, final_text)
        .ops()
        .iter()
        .filter(|operation| operation.tag() != DiffTag::Equal)
        .map(|operation| {
            let old_range = operation.old_range();
            let new_range = operation.new_range();
            DiffRange {
                old_start: old_range.start,
                old_end: old_range.end,
                new_start: new_range.start,
                new_end: new_range.end,
                local_candidate: ascii_candidate(&final_chars, new_range.start, new_range.end),
            }
        })
        .collect::<Vec<_>>();
    merge_same_ascii_candidate(raw)
        .into_iter()
        .map(|range| TextRevisionDiff {
            before: original_chars[range.old_start..range.old_end]
                .iter()
                .collect(),
            after: final_chars[range.new_start..range.new_end].iter().collect(),
            before_context: context(&original_chars, range.old_start, range.old_end),
            after_context: context(&final_chars, range.new_start, range.new_end),
            local_candidate: range.local_candidate,
        })
        .collect()
}

struct DiffRange {
    old_start: usize,
    old_end: usize,
    new_start: usize,
    new_end: usize,
    local_candidate: Option<String>,
}

fn merge_same_ascii_candidate(ranges: Vec<DiffRange>) -> Vec<DiffRange> {
    let mut merged: Vec<DiffRange> = Vec::new();
    for range in ranges {
        if let Some(previous) = merged.last_mut()
            && range.local_candidate.is_some()
            && range.local_candidate == previous.local_candidate
        {
            previous.old_end = range.old_end;
            previous.new_end = range.new_end;
            continue;
        }
        merged.push(range);
    }
    merged
}

fn context(chars: &[char], start: usize, end: usize) -> String {
    chars[start.saturating_sub(DIFF_CONTEXT_CHARS)
        ..end.saturating_add(DIFF_CONTEXT_CHARS).min(chars.len())]
        .iter()
        .collect()
}

fn ascii_candidate(chars: &[char], changed_start: usize, changed_end: usize) -> Option<String> {
    if changed_start == changed_end {
        return None;
    }
    let mut start = changed_start;
    while start > 0 && is_ascii_term_character(chars[start - 1]) {
        start -= 1;
    }
    let mut end = changed_end;
    while end < chars.len() && is_ascii_term_character(chars[end]) {
        end += 1;
    }
    let value = chars[start..end].iter().collect::<String>();
    (value.chars().count() >= 2 && value.chars().all(is_ascii_term_character)).then_some(value)
}

fn is_ascii_term_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

/// Returns whether an unanchored whole-control change still has enough textual
/// continuity to be treated as a user revision of the delivered text.
pub fn has_text_revision_continuity(original: &str, edited: &str) -> bool {
    let original = original.chars().collect::<Vec<_>>();
    let edited = edited.chars().collect::<Vec<_>>();
    let prefix = original
        .iter()
        .zip(&edited)
        .take_while(|(left, right)| left == right)
        .count();
    let suffix = original[prefix..]
        .iter()
        .rev()
        .zip(edited[prefix..].iter().rev())
        .take_while(|(left, right)| left == right)
        .count();
    let retained = prefix.saturating_add(suffix);
    let longest = original.len().max(edited.len());
    retained > 0 && (longest <= 3 || retained.saturating_mul(3) >= longest.saturating_mul(2))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_last_snapshot_is_committed_when_observation_ends() {
        let mut state = FinalTextRevisionState::new("原始文本");
        assert_eq!(
            None,
            state.handle(TextRevisionEvent::Snapshot("修改一".into()))
        );
        assert_eq!(
            None,
            state.handle(TextRevisionEvent::Snapshot("最终修改".into()))
        );

        let revision = state.handle(TextRevisionEvent::Ended(
            TextRevisionEndReason::ControlReset,
        ));

        assert_eq!(
            Some(FinalTextRevision {
                original: "原始文本".into(),
                final_text: "最终修改".into(),
                end_reason: TextRevisionEndReason::ControlReset,
            }),
            revision
        );
        assert_eq!(
            None,
            state.handle(TextRevisionEvent::Snapshot("过晚".into()))
        );
    }

    #[test]
    fn timeout_updates_final_text_without_permitting_dictionary_learning() {
        let mut state = FinalTextRevisionState::new("原始文本");
        state.handle(TextRevisionEvent::Snapshot("超时前修改".into()));

        let revision = state
            .handle(TextRevisionEvent::Ended(TextRevisionEndReason::TimedOut))
            .unwrap_or_else(|| unreachable!("a changed snapshot must produce a revision"));

        assert_eq!("超时前修改", revision.final_text);
        assert!(!revision.permits_dictionary_learning());
    }

    #[test]
    fn character_diff_keeps_shared_context_for_a_chinese_term() {
        assert_eq!(
            vec![TextRevisionDiff {
                before: "万".into(),
                after: "问".into(),
                before_context: "我在用通义千万模型".into(),
                after_context: "我在用通义千问模型".into(),
                local_candidate: None,
            }],
            text_revision_diffs("我在用通义千万模型", "我在用通义千问模型")
        );
    }

    #[test]
    fn character_diff_reports_multiple_non_contiguous_edits() {
        let diffs = text_revision_diffs("使用 slint 和 jason", "使用 SlintUI 和 JSON");

        assert_eq!(2, diffs.len());
        assert_eq!(Some("SlintUI"), diffs[0].local_candidate.as_deref());
        assert_eq!(Some("JSON"), diffs[1].local_candidate.as_deref());
    }

    #[test]
    fn whole_control_resets_do_not_look_like_user_revisions() {
        assert!(has_text_revision_continuity("千万", "千问"));
        assert!(!has_text_revision_continuity(
            "为什么会识别别成这个随便输入",
            "随心输入"
        ));
        assert!(has_text_revision_continuity(
            "为什么会识别别成这个随便输入",
            "为什么会识别别成这个随心输入"
        ));
    }
}
