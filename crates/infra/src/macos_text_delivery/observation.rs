use std::{
    thread,
    time::{Duration, Instant},
};

use accessibility_sys::{
    kAXFocusedAttribute, kAXNumberOfCharactersAttribute, kAXSecureTextFieldSubrole,
    kAXSubroleAttribute,
};
use template_app::{ObservedTextEdit, TextEditObserver};

use super::{OwnedAxElement, TextRange, secure_event_input_enabled};

const POLL_INTERVAL: Duration = Duration::from_millis(200);
const STABLE_DELAY: Duration = Duration::from_millis(1_200);
const OBSERVATION_TIMEOUT: Duration = Duration::from_secs(30);
const ANCHOR_UNITS: usize = 24;
const MAX_GROWTH_UNITS: usize = 64;

pub(super) struct CorrectionObservationTarget {
    focused: OwnedAxElement,
    original: String,
    original_range: TextRange,
    initial_character_count: usize,
    prefix: String,
    suffix: String,
}

impl CorrectionObservationTarget {
    pub(super) fn capture(
        focused: OwnedAxElement,
        original_range: TextRange,
        text: &str,
    ) -> Option<Self> {
        if secure_event_input_enabled()
            || focused.attribute_bool(kAXFocusedAttribute).ok().flatten() != Some(true)
        {
            return None;
        }
        match focused.attribute_string(kAXSubroleAttribute) {
            Ok(Some(subrole)) if subrole != kAXSecureTextFieldSubrole => {}
            Ok(Some(_)) | Ok(None) | Err(_) => return None,
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
        })
    }

    pub(super) fn observe(self, observer: TextEditObserver) {
        let deadline = Instant::now() + OBSERVATION_TIMEOUT;
        let mut last_edit: Option<(String, Instant)> = None;
        loop {
            if secure_event_input_enabled() {
                return;
            }
            let focused = self
                .focused
                .attribute_bool(kAXFocusedAttribute)
                .ok()
                .flatten();
            let edited = self.current_text();
            if let Some(edited) = edited.filter(|edited| edited != &self.original) {
                if focused == Some(false) {
                    observer(ObservedTextEdit {
                        original: self.original,
                        edited,
                    });
                    return;
                }
                match &mut last_edit {
                    Some((previous, changed_at)) if previous == &edited => {
                        if changed_at.elapsed() >= STABLE_DELAY {
                            observer(ObservedTextEdit {
                                original: self.original,
                                edited,
                            });
                            return;
                        }
                    }
                    Some((previous, changed_at)) => {
                        *previous = edited;
                        *changed_at = Instant::now();
                    }
                    None => last_edit = Some((edited, Instant::now())),
                }
            } else {
                last_edit = None;
            }
            if focused == Some(false) || Instant::now() >= deadline {
                return;
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    fn current_text(&self) -> Option<String> {
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
        text_between_anchors(&window, &self.prefix, &self.suffix)
    }
}

pub(super) fn text_between_anchors(window: &str, prefix: &str, suffix: &str) -> Option<String> {
    let after_prefix = window.strip_prefix(prefix)?;
    if suffix.is_empty() {
        return Some(after_prefix.to_owned());
    }
    let suffix_start = after_prefix.rfind(suffix)?;
    Some(after_prefix[..suffix_start].to_owned())
}
