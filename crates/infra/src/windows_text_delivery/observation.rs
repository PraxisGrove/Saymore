use std::time::{Duration, Instant};

use template_app::{
    TextRevisionEndReason, TextRevisionEvent, TextRevisionObserver, has_text_revision_continuity,
};

use windows::Win32::UI::Accessibility::{IUIAutomationTextEditPattern, UIA_TextEditPatternId};

use super::{FocusedTarget, current_pattern, observable_control_text};

pub(super) const POLL_INTERVAL: Duration = Duration::from_millis(200);
const OBSERVATION_TIMEOUT: Duration = Duration::from_secs(2 * 60);
const ANCHOR_CHARACTERS: usize = 24;

enum ObservedControlText {
    Content(String),
    Reset,
}

pub(super) struct CorrectionObservationTarget {
    focused: windows::Win32::UI::Accessibility::IUIAutomationElement,
    original: String,
    prefix: String,
    suffix: String,
    reset_text: String,
}

impl CorrectionObservationTarget {
    pub(super) fn capture(target: &FocusedTarget, original: &str) -> Option<Self> {
        if target.sensitive {
            return None;
        }
        let before = target.initial_text.as_deref()?;
        let after = observable_control_text(&target.element)?;
        let (prefix, suffix) = insertion_anchors(before, &after, original)?;
        Some(Self {
            focused: target.element.clone(),
            original: original.to_owned(),
            prefix,
            suffix,
            reset_text: before.to_owned(),
        })
    }

    fn current_text(&self) -> Option<ObservedControlText> {
        let current = observable_control_text(&self.focused)?;
        if current.trim().is_empty() || current == self.reset_text {
            return Some(ObservedControlText::Reset);
        }
        let edited = text_between_anchors(&current, &self.prefix, &self.suffix)?;
        let anchored = !self.prefix.is_empty() || !self.suffix.is_empty();
        if edited.trim().is_empty()
            || (!anchored && !has_text_revision_continuity(&self.original, &edited))
        {
            Some(ObservedControlText::Reset)
        } else {
            Some(ObservedControlText::Content(edited))
        }
    }

    fn has_focus(&self) -> Option<bool> {
        unsafe { self.focused.CurrentHasKeyboardFocus() }
            .ok()
            .map(|focused| focused.as_bool())
    }

    fn has_active_composition(&self) -> bool {
        current_pattern::<IUIAutomationTextEditPattern>(&self.focused, UIA_TextEditPatternId)
            .ok()
            .flatten()
            .and_then(|pattern| unsafe { pattern.GetActiveComposition().ok() })
            .and_then(|range| unsafe { range.GetText(-1).ok() })
            .is_some_and(|text| !text.is_empty())
    }
}

pub(super) struct ActiveCorrectionObservation {
    target: CorrectionObservationTarget,
    observer: TextRevisionObserver,
    deadline: Instant,
    last_reported: String,
    ended: bool,
}

impl ActiveCorrectionObservation {
    pub(super) fn new(target: CorrectionObservationTarget, observer: TextRevisionObserver) -> Self {
        Self {
            last_reported: target.original.clone(),
            target,
            observer,
            deadline: Instant::now() + OBSERVATION_TIMEOUT,
            ended: false,
        }
    }

    /// Returns true once this observation no longer needs polling.
    pub(super) fn poll(&mut self) -> bool {
        if self.ended {
            return true;
        }
        let focused = self.target.has_focus();
        let has_active_composition = self.target.has_active_composition();
        let edited = match self.target.current_text() {
            Some(ObservedControlText::Content(edited)) => Some(edited),
            Some(ObservedControlText::Reset) if has_active_composition => None,
            Some(ObservedControlText::Reset) => {
                self.finish(TextRevisionEndReason::ControlReset);
                return true;
            }
            None => None,
        };

        if let Some(edited) = edited
            && !has_active_composition
        {
            self.report_if_changed(&edited);
        }
        if focused == Some(false) {
            self.finish(TextRevisionEndReason::FocusLost);
            return true;
        }
        if Instant::now() >= self.deadline {
            self.finish(TextRevisionEndReason::TimedOut);
            return true;
        }
        false
    }

    fn report_if_changed(&mut self, edited: &str) {
        if edited != self.last_reported {
            (self.observer)(TextRevisionEvent::Snapshot(edited.to_owned()));
            edited.clone_into(&mut self.last_reported);
        }
    }

    pub(super) fn finish(&mut self, reason: TextRevisionEndReason) {
        if !self.ended {
            self.ended = true;
            (self.observer)(TextRevisionEvent::Ended(reason));
        }
    }
}

fn insertion_anchors(before: &str, after: &str, original: &str) -> Option<(String, String)> {
    let before = before.chars().collect::<Vec<_>>();
    let after = after.chars().collect::<Vec<_>>();
    let original = original.chars().collect::<Vec<_>>();

    let common_prefix = before
        .iter()
        .zip(&after)
        .take_while(|(left, right)| left == right)
        .count();
    let suffix_limit = before
        .len()
        .saturating_sub(common_prefix)
        .min(after.len().saturating_sub(common_prefix));
    let common_suffix = before
        .iter()
        .rev()
        .zip(after.iter().rev())
        .take(suffix_limit)
        .take_while(|(left, right)| left == right)
        .count();
    let inserted_end = after.len().checked_sub(common_suffix)?;
    if after.get(common_prefix..inserted_end)? != original.as_slice() {
        return None;
    }

    let prefix_start = common_prefix.saturating_sub(ANCHOR_CHARACTERS);
    let suffix_end = inserted_end
        .checked_add(ANCHOR_CHARACTERS)?
        .min(after.len());
    Some((
        after[prefix_start..common_prefix].iter().collect(),
        after[inserted_end..suffix_end].iter().collect(),
    ))
}

fn text_between_anchors(text: &str, prefix: &str, suffix: &str) -> Option<String> {
    let start = if prefix.is_empty() {
        0
    } else {
        text.rfind(prefix)?.checked_add(prefix.len())?
    };
    let after_prefix = text.get(start..)?;
    if suffix.is_empty() {
        return Some(after_prefix.trim().to_owned());
    }
    let suffix_start = after_prefix.rfind(suffix)?;
    Some(after_prefix.get(..suffix_start)?.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchors_capture_unicode_insertion() {
        assert_eq!(
            Some(("你好".to_owned(), "世界".to_owned())),
            insertion_anchors("你好世界", "你好🌟世界", "🌟")
        );
    }

    #[test]
    fn anchors_capture_selected_text_replacement() {
        assert_eq!(
            Some(("before ".to_owned(), " after".to_owned())),
            insertion_anchors("before old after", "before new after", "new")
        );
    }

    #[test]
    fn unrelated_change_is_not_observed() {
        assert_eq!(None, insertion_anchors("before", "after", "dictation"));
    }

    #[test]
    fn edited_text_is_recovered_between_anchors() {
        assert_eq!(
            Some("corrected words".to_owned()),
            text_between_anchors(
                "surrounding corrected words remaining",
                "surrounding ",
                " remaining"
            )
        );
    }
}
