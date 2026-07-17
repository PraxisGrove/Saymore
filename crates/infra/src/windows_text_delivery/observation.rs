use std::time::{Duration, Instant};

use template_app::{ObservedTextEdit, TextEditObserver};

use super::{FocusedTarget, observable_control_text};

pub(super) const POLL_INTERVAL: Duration = Duration::from_millis(200);
const STABLE_DELAY: Duration = Duration::from_millis(1_200);
const OBSERVATION_TIMEOUT: Duration = Duration::from_secs(30);
const ANCHOR_CHARACTERS: usize = 24;

pub(super) struct CorrectionObservationTarget {
    focused: windows::Win32::UI::Accessibility::IUIAutomationElement,
    original: String,
    prefix: String,
    suffix: String,
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
        })
    }

    fn current_text(&self) -> Option<String> {
        let current = observable_control_text(&self.focused)?;
        text_between_anchors(&current, &self.prefix, &self.suffix)
    }

    fn has_focus(&self) -> Option<bool> {
        unsafe { self.focused.CurrentHasKeyboardFocus() }
            .ok()
            .map(|focused| focused.as_bool())
    }
}

pub(super) struct ActiveCorrectionObservation {
    target: CorrectionObservationTarget,
    observer: Option<TextEditObserver>,
    deadline: Instant,
    last_edit: Option<(String, Instant)>,
}

impl ActiveCorrectionObservation {
    pub(super) fn new(target: CorrectionObservationTarget, observer: TextEditObserver) -> Self {
        Self {
            target,
            observer: Some(observer),
            deadline: Instant::now() + OBSERVATION_TIMEOUT,
            last_edit: None,
        }
    }

    /// Returns true once this observation no longer needs polling.
    pub(super) fn poll(&mut self) -> bool {
        let focused = self.target.has_focus();
        let edited = self
            .target
            .current_text()
            .filter(|edited| edited != &self.target.original);

        if let Some(edited) = edited {
            if focused == Some(false) {
                self.report(edited);
                return true;
            }
            match &mut self.last_edit {
                Some((previous, changed_at)) if previous == &edited => {
                    if changed_at.elapsed() >= STABLE_DELAY {
                        self.report(edited);
                        return true;
                    }
                }
                Some((previous, changed_at)) => {
                    *previous = edited;
                    *changed_at = Instant::now();
                }
                None => self.last_edit = Some((edited, Instant::now())),
            }
        } else {
            self.last_edit = None;
        }

        focused == Some(false) || Instant::now() >= self.deadline
    }

    fn report(&mut self, edited: String) {
        if let Some(observer) = self.observer.take() {
            observer(ObservedTextEdit {
                original: self.target.original.clone(),
                edited,
            });
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
        return Some(after_prefix.to_owned());
    }
    let suffix_start = after_prefix.rfind(suffix)?;
    Some(after_prefix.get(..suffix_start)?.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchors_capture_unicode_insertion() {
        assert_eq!(
            Some(("你好".to_owned(), "世界".to_owned())),
            insertion_anchors("你好世界", "你好，Saymore 世界", ", Saymore")
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
