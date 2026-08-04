use std::{
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use accessibility_sys::{
    kAXComboBoxRole, kAXFocusedAttribute, kAXNumberOfCharactersAttribute,
    kAXPlaceholderValueAttribute, kAXRoleAttribute, kAXSecureTextFieldSubrole, kAXSubroleAttribute,
    kAXTextAreaRole, kAXTextFieldRole, kAXValueAttribute,
};
use template_app::{
    TextRevisionEndReason, TextRevisionEvent, TextRevisionObserver, has_text_revision_continuity,
};

use super::{OwnedAxElement, TextRange, secure_event_input_enabled};

const POLL_INTERVAL: Duration = Duration::from_millis(200);
const OBSERVATION_TIMEOUT: Duration = Duration::from_secs(2 * 60);
const ANCHOR_UNITS: usize = 24;
const MAX_GROWTH_UNITS: usize = 64;
const MAX_RESET_VALUE_CHARS: usize = 128;
const AX_TEXT_INPUT_MARKED_RANGE_ATTRIBUTE: &str = "AXTextInputMarkedRange";
const AX_TEXT_INPUT_MARKED_TEXT_MARKER_RANGE_ATTRIBUTE: &str = "AXTextInputMarkedTextMarkerRange";

enum ObservedControlText {
    Content(String),
    Reset,
}

pub(super) struct ControlResetSnapshot {
    value: Option<String>,
}

impl ControlResetSnapshot {
    pub(super) fn capture(focused: &OwnedAxElement) -> Self {
        let value = focused
            .attribute_string(kAXValueAttribute)
            .ok()
            .flatten()
            .filter(|value| value.chars().count() <= MAX_RESET_VALUE_CHARS);
        Self { value }
    }

    fn value(&self) -> Option<&str> {
        self.value.as_deref()
    }
}

pub(super) struct CorrectionObservationTarget {
    focused: OwnedAxElement,
    original: String,
    original_range: TextRange,
    initial_character_count: usize,
    prefix: String,
    suffix: String,
    reset_snapshot: ControlResetSnapshot,
}

impl CorrectionObservationTarget {
    pub(super) fn capture(
        focused: OwnedAxElement,
        original_range: TextRange,
        text: &str,
        reset_snapshot: ControlResetSnapshot,
    ) -> Option<Self> {
        if secure_event_input_enabled()
            || focused.attribute_bool(kAXFocusedAttribute).ok().flatten() != Some(true)
        {
            return None;
        }
        let role = focused.attribute_string(kAXRoleAttribute).ok().flatten();
        let subrole = focused.attribute_string(kAXSubroleAttribute).ok().flatten();
        if !observable_text_control(role.as_deref(), subrole.as_deref()) {
            return None;
        }
        let initial_character_count = focused
            .attribute_usize(kAXNumberOfCharactersAttribute)
            .ok()
            .flatten()?;
        let original_units = text.encode_utf16().count();
        let prefix_units = original_range.location.min(ANCHOR_UNITS);
        let suffix_start = original_range.location.checked_add(original_units)?;
        let suffix_units = initial_character_count
            .saturating_sub(suffix_start)
            .min(ANCHOR_UNITS);
        let prefix = focused
            .string_for_range(TextRange {
                location: original_range.location.saturating_sub(prefix_units),
                length: prefix_units,
            })
            .ok()
            .flatten()
            .unwrap_or_default();
        let suffix = focused
            .string_for_range(TextRange {
                location: suffix_start,
                length: suffix_units,
            })
            .ok()
            .flatten()
            .unwrap_or_default();
        Some(Self {
            focused,
            original: text.to_owned(),
            original_range,
            initial_character_count,
            prefix,
            suffix,
            reset_snapshot,
        })
    }

    pub(super) fn observe(
        self,
        observer: TextRevisionObserver,
        cancelled: mpsc::Receiver<TextRevisionEndReason>,
    ) {
        let deadline = Instant::now() + OBSERVATION_TIMEOUT;
        let mut latest_revision = None;
        loop {
            if let Ok(reason) = cancelled.try_recv() {
                observer(TextRevisionEvent::Ended(reason));
                return;
            }
            if secure_event_input_enabled() {
                observer(TextRevisionEvent::Ended(TextRevisionEndReason::Cancelled));
                return;
            }
            let focused = self
                .focused
                .attribute_bool(kAXFocusedAttribute)
                .ok()
                .flatten();
            let observed = self.current_text();
            let marked_text = self
                .focused
                .attribute_text_range(AX_TEXT_INPUT_MARKED_RANGE_ATTRIBUTE)
                .ok()
                .flatten()
                .is_some_and(|range| range.length > 0)
                || self
                    .focused
                    .has_attribute_value(AX_TEXT_INPUT_MARKED_TEXT_MARKER_RANGE_ATTRIBUTE)
                    .unwrap_or(false);
            match observed {
                Some(ObservedControlText::Content(edited)) if !marked_text => {
                    if edited != self.original
                        && latest_revision.as_deref() != Some(edited.as_str())
                    {
                        observer(TextRevisionEvent::Snapshot(edited.clone()));
                        latest_revision = Some(edited);
                    }
                }
                Some(ObservedControlText::Reset) => {
                    observer(TextRevisionEvent::Ended(
                        TextRevisionEndReason::ControlReset,
                    ));
                    return;
                }
                Some(ObservedControlText::Content(_)) | None => {}
            }
            if focused == Some(false) {
                observer(TextRevisionEvent::Ended(TextRevisionEndReason::FocusLost));
                return;
            }
            if Instant::now() >= deadline {
                observer(TextRevisionEvent::Ended(TextRevisionEndReason::TimedOut));
                return;
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    fn current_text(&self) -> Option<ObservedControlText> {
        let current_count = self
            .focused
            .attribute_usize(kAXNumberOfCharactersAttribute)
            .ok()
            .flatten()?;
        let prefix_units = self.prefix.encode_utf16().count();
        let suffix_units = self.suffix.encode_utf16().count();
        let window_start = self.original_range.location.saturating_sub(prefix_units);
        let original_units = self.original.encode_utf16().count();
        let count_delta = current_count.saturating_sub(self.initial_character_count);
        let desired_length = prefix_units
            .checked_add(original_units)?
            .checked_add(count_delta.min(MAX_GROWTH_UNITS))?
            .checked_add(suffix_units)?;
        let available = current_count.saturating_sub(window_start);
        let window = self
            .focused
            .string_for_range(TextRange {
                location: window_start,
                length: desired_length.min(available),
            })
            .ok()
            .flatten()?;
        let edited = text_between_anchors(&window, &self.prefix, &self.suffix)?;
        let placeholder = self
            .focused
            .attribute_string(kAXPlaceholderValueAttribute)
            .ok()
            .flatten();
        let anchored = !self.prefix.is_empty() || !self.suffix.is_empty();
        if is_input_reset(
            &self.original,
            &edited,
            anchored,
            placeholder.as_deref(),
            self.reset_snapshot.value(),
        ) {
            Some(ObservedControlText::Reset)
        } else {
            Some(ObservedControlText::Content(edited))
        }
    }
}

fn is_input_reset(
    original: &str,
    edited: &str,
    anchored: bool,
    placeholder: Option<&str>,
    reset_value: Option<&str>,
) -> bool {
    edited.trim().is_empty()
        || placeholder.is_some_and(|placeholder| edited == placeholder)
        || reset_value.is_some_and(|reset_value| edited == reset_value)
        || (!anchored && !has_text_revision_continuity(original, edited))
}

pub(super) fn observable_text_control(role: Option<&str>, subrole: Option<&str>) -> bool {
    if subrole == Some(kAXSecureTextFieldSubrole) {
        return false;
    }
    role.is_some_and(|role| {
        role == kAXTextAreaRole || role == kAXTextFieldRole || role == kAXComboBoxRole
    })
}

pub(super) fn text_between_anchors(window: &str, prefix: &str, suffix: &str) -> Option<String> {
    let after_prefix = window.strip_prefix(prefix)?;
    if suffix.is_empty() {
        return Some(after_prefix.trim().to_owned());
    }
    let suffix_start = after_prefix.rfind(suffix)?;
    Some(after_prefix[..suffix_start].trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_reset_text_is_not_reported_as_a_correction() {
        assert!(is_input_reset("原始文本", "", false, None, None));
        assert!(is_input_reset(
            "原始文本",
            "随心输入",
            false,
            Some("随心输入"),
            None,
        ));
        assert!(is_input_reset(
            "为什么会识别别成这个随便输入",
            "随心输入",
            false,
            None,
            None,
        ));
        assert!(is_input_reset(
            "还有我们怎么能不带火山引擎这个大语言模型呢",
            "随心输入",
            false,
            None,
            None,
        ));
        assert!(!is_input_reset(
            "为什么会识别别成这个随便输入",
            "为什么会识别别成这个随心输入",
            false,
            None,
            None,
        ));
        assert!(!is_input_reset("原始文本", "完全重写", true, None, None,));
        assert!(is_input_reset(
            "原始文本",
            "随心输入",
            true,
            None,
            Some("随心输入"),
        ));
    }
}
