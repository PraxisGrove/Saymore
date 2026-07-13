use std::{ffi::c_void, ptr, thread, time::Duration, time::Instant};

use accessibility_sys::{
    AXIsProcessTrusted, AXIsProcessTrustedWithOptions, AXUIElementCopyAttributeValue,
    AXUIElementCreateSystemWide, AXUIElementIsAttributeSettable, AXUIElementRef,
    AXUIElementSetAttributeValue, AXValueGetType, AXValueGetTypeID, AXValueGetValue, AXValueRef,
    error_string, kAXErrorAttributeUnsupported, kAXErrorNoValue, kAXErrorSuccess,
    kAXFocusedUIElementAttribute, kAXSecureTextFieldSubrole, kAXSelectedTextAttribute,
    kAXSelectedTextRangeAttribute, kAXSubroleAttribute, kAXTrustedCheckOptionPrompt,
    kAXValueTypeCFRange,
};
use core_foundation::{
    base::TCFType, boolean::CFBoolean, dictionary::CFDictionary, string::CFString,
};
use core_foundation_sys::{
    base::{CFGetTypeID, CFRange, CFRelease, CFTypeRef},
    string::{CFStringGetTypeID, CFStringRef},
};
use core_graphics::{
    event::{CGEvent, CGEventFlags, CGEventTapLocation},
    event_source::{CGEventSource, CGEventSourceStateID},
};
use objc2::{rc::Retained, runtime::ProtocolObject};
use objc2_app_kit::{NSPasteboard, NSPasteboardItem, NSPasteboardWriting};
use objc2_foundation::{NSArray, NSData, NSString};
use template_app::{
    AccessibilityAuthorization, TextDeliverer, TextDeliveryError, TextDeliveryOutcome,
};

const COMMAND_KEY_CODE: u16 = 55;
const V_KEY_CODE: u16 = 9;
const VERIFICATION_POLL_INTERVAL: Duration = Duration::from_millis(20);
const ACCESSIBILITY_VERIFICATION_TIMEOUT: Duration = Duration::from_millis(180);
const PASTE_VERIFICATION_TIMEOUT: Duration = Duration::from_millis(700);

#[derive(Debug, Clone, Copy, Default)]
pub struct MacOsTextDeliverer;

/// Replaces the macOS clipboard with a transcript the user explicitly chose
/// to copy from the recovery overlay.
pub fn copy_text_to_clipboard(text: &str) -> Result<(), TextDeliveryError> {
    let pasteboard = NSPasteboard::generalPasteboard();
    if write_text(&pasteboard, text) {
        Ok(())
    } else {
        Err(TextDeliveryError::System(
            "failed to copy transcript to the pasteboard".to_owned(),
        ))
    }
}

impl TextDeliverer for MacOsTextDeliverer {
    fn authorization(&self) -> AccessibilityAuthorization {
        authorization_from(unsafe { AXIsProcessTrusted() })
    }

    fn request_authorization(&self) -> AccessibilityAuthorization {
        let prompt_key = unsafe { CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt) };
        let options = CFDictionary::from_CFType_pairs(&[(prompt_key, CFBoolean::true_value())]);
        authorization_from(unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef()) })
    }

    fn deliver(&self, text: &str) -> Result<TextDeliveryOutcome, TextDeliveryError> {
        if self.authorization() == AccessibilityAuthorization::Denied {
            return Err(TextDeliveryError::PermissionDenied);
        }

        let system = OwnedAxElement::system_wide()?;
        let focused = system.focused_control()?;

        if focused.attribute_string(kAXSubroleAttribute)?.as_deref()
            == Some(kAXSecureTextFieldSubrole)
        {
            return Err(TextDeliveryError::SecureInput);
        }

        let initial_range = focused.selected_text_range().ok().flatten();
        let Some(initial_range) = initial_range else {
            return paste_with_clipboard(&focused, None, text);
        };

        match focused.replace_selection(text) {
            Ok(()) => match verify_insertion(
                &focused,
                initial_range,
                text,
                ACCESSIBILITY_VERIFICATION_TIMEOUT,
            ) {
                InsertionVerification::Verified => Ok(TextDeliveryOutcome::AccessibilityVerified),
                InsertionVerification::Unchanged => {
                    paste_with_clipboard(&focused, Some(initial_range), text)
                }
                InsertionVerification::Unverified => {
                    Err(TextDeliveryError::AccessibilityUnverified)
                }
            },
            Err(TextDeliveryError::UnsupportedControl | TextDeliveryError::System(_)) => {
                paste_with_clipboard(&focused, Some(initial_range), text)
            }
            Err(error) => Err(error),
        }
    }
}

fn authorization_from(trusted: bool) -> AccessibilityAuthorization {
    if trusted {
        AccessibilityAuthorization::Granted
    } else {
        AccessibilityAuthorization::Denied
    }
}

struct OwnedAxElement(AXUIElementRef);

impl OwnedAxElement {
    fn system_wide() -> Result<Self, TextDeliveryError> {
        let element = unsafe { AXUIElementCreateSystemWide() };
        if element.is_null() {
            Err(TextDeliveryError::System(
                "AXUIElementCreateSystemWide returned null".to_owned(),
            ))
        } else {
            Ok(Self(element))
        }
    }

    fn focused_control(&self) -> Result<Self, TextDeliveryError> {
        let attribute = CFString::new(kAXFocusedUIElementAttribute);
        let mut value: CFTypeRef = ptr::null();
        let error = unsafe {
            AXUIElementCopyAttributeValue(self.0, attribute.as_concrete_TypeRef(), &mut value)
        };

        if error == kAXErrorNoValue || value.is_null() {
            return Err(TextDeliveryError::NoFocusedControl);
        }
        if error != kAXErrorSuccess {
            return Err(system_error("read focused control", error));
        }

        Ok(Self(value as AXUIElementRef))
    }

    fn attribute_string(&self, name: &str) -> Result<Option<String>, TextDeliveryError> {
        let attribute = CFString::new(name);
        let mut value: CFTypeRef = ptr::null();
        let error = unsafe {
            AXUIElementCopyAttributeValue(self.0, attribute.as_concrete_TypeRef(), &mut value)
        };

        if error == kAXErrorNoValue || value.is_null() {
            return Ok(None);
        }
        if error != kAXErrorSuccess {
            return Err(system_error("read control attribute", error));
        }

        if unsafe { CFGetTypeID(value) } != unsafe { CFStringGetTypeID() } {
            unsafe { CFRelease(value) };
            return Ok(None);
        }

        let value = unsafe { CFString::wrap_under_create_rule(value.cast::<_>() as CFStringRef) };
        Ok(Some(value.to_string()))
    }

    fn replace_selection(&self, text: &str) -> Result<(), TextDeliveryError> {
        let attribute = CFString::new(kAXSelectedTextAttribute);
        let mut settable = 0;
        let error = unsafe {
            AXUIElementIsAttributeSettable(self.0, attribute.as_concrete_TypeRef(), &mut settable)
        };

        if error == kAXErrorAttributeUnsupported {
            return Err(TextDeliveryError::UnsupportedControl);
        }
        if error != kAXErrorSuccess {
            return Err(system_error("inspect selected text", error));
        }
        if settable == 0 {
            return Err(TextDeliveryError::UnsupportedControl);
        }

        let text = CFString::new(text);
        let error = unsafe {
            AXUIElementSetAttributeValue(
                self.0,
                attribute.as_concrete_TypeRef(),
                text.as_CFTypeRef(),
            )
        };

        if error == kAXErrorSuccess {
            Ok(())
        } else {
            Err(system_error("replace selected text", error))
        }
    }

    fn selected_text_range(&self) -> Result<Option<TextRange>, TextDeliveryError> {
        let attribute = CFString::new(kAXSelectedTextRangeAttribute);
        let mut value: CFTypeRef = ptr::null();
        let error = unsafe {
            AXUIElementCopyAttributeValue(self.0, attribute.as_concrete_TypeRef(), &mut value)
        };

        if error == kAXErrorAttributeUnsupported || error == kAXErrorNoValue || value.is_null() {
            return Ok(None);
        }
        if error != kAXErrorSuccess {
            return Err(system_error("read selected text range", error));
        }

        let range = read_cf_range(value);
        unsafe { CFRelease(value) };
        Ok(range)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TextRange {
    location: usize,
    length: usize,
}

impl TextRange {
    fn from_cf_range(range: CFRange) -> Option<Self> {
        Some(Self {
            location: usize::try_from(range.location).ok()?,
            length: usize::try_from(range.length).ok()?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InsertionVerification {
    Verified,
    Unchanged,
    Unverified,
}

fn read_cf_range(value: CFTypeRef) -> Option<TextRange> {
    if unsafe { CFGetTypeID(value) } != unsafe { AXValueGetTypeID() } {
        return None;
    }

    let value = value.cast_mut().cast::<accessibility_sys::__AXValue>();
    if unsafe { AXValueGetType(value) } != kAXValueTypeCFRange {
        return None;
    }

    let mut range = CFRange::init(0, 0);
    let read = unsafe {
        AXValueGetValue(
            value as AXValueRef,
            kAXValueTypeCFRange,
            ptr::from_mut(&mut range).cast::<c_void>(),
        )
    };
    read.then(|| TextRange::from_cf_range(range)).flatten()
}

fn verify_insertion(
    focused: &OwnedAxElement,
    initial: TextRange,
    text: &str,
    timeout: Duration,
) -> InsertionVerification {
    let deadline = Instant::now() + timeout;
    let mut range_changed = false;

    loop {
        match focused.selected_text_range() {
            Ok(Some(current)) if insertion_range_matches(initial, current, text) => {
                return InsertionVerification::Verified;
            }
            Ok(Some(current)) => range_changed |= current != initial,
            Ok(None) | Err(_) => return InsertionVerification::Unverified,
        }

        if Instant::now() >= deadline {
            return if range_changed {
                InsertionVerification::Unverified
            } else {
                InsertionVerification::Unchanged
            };
        }
        thread::sleep(VERIFICATION_POLL_INTERVAL);
    }
}

fn insertion_range_matches(initial: TextRange, current: TextRange, text: &str) -> bool {
    let inserted_length = text.encode_utf16().count();
    let collapsed_location = initial.location.checked_add(inserted_length);
    let collapsed_after_text = current.length == 0 && Some(current.location) == collapsed_location;
    let inserted_text_selected =
        current.location == initial.location && current.length == inserted_length;
    collapsed_after_text || inserted_text_selected
}

fn paste_with_clipboard(
    focused: &OwnedAxElement,
    initial_range: Option<TextRange>,
    text: &str,
) -> Result<TextDeliveryOutcome, TextDeliveryError> {
    let pasteboard = NSPasteboard::generalPasteboard();
    let snapshot = PasteboardSnapshot::capture(&pasteboard);

    if !write_text(&pasteboard, text) {
        return Err(TextDeliveryError::System(
            "failed to write temporary pasteboard text".to_owned(),
        ));
    }

    let temporary_change_count = pasteboard.changeCount();
    if let Err(error) = post_paste_shortcut() {
        restore_if_unchanged(snapshot, &pasteboard, temporary_change_count)?;
        return Err(error);
    }

    let outcome = match initial_range {
        Some(initial_range) => {
            match verify_insertion(focused, initial_range, text, PASTE_VERIFICATION_TIMEOUT) {
                InsertionVerification::Verified => Ok(TextDeliveryOutcome::ClipboardVerified),
                InsertionVerification::Unchanged | InsertionVerification::Unverified => {
                    Ok(TextDeliveryOutcome::ClipboardAttempted)
                }
            }
        }
        None => {
            thread::sleep(PASTE_VERIFICATION_TIMEOUT);
            Ok(TextDeliveryOutcome::ClipboardAttempted)
        }
    };

    restore_if_unchanged(snapshot, &pasteboard, temporary_change_count)?;
    outcome
}

fn restore_if_unchanged(
    snapshot: PasteboardSnapshot,
    pasteboard: &NSPasteboard,
    temporary_change_count: isize,
) -> Result<(), TextDeliveryError> {
    if pasteboard.changeCount() == temporary_change_count {
        snapshot.restore(pasteboard)
    } else {
        Ok(())
    }
}

fn write_text(pasteboard: &NSPasteboard, text: &str) -> bool {
    let text = NSString::from_str(text);
    let text: Retained<ProtocolObject<dyn NSPasteboardWriting>> =
        ProtocolObject::from_retained(text);
    let objects = NSArray::from_retained_slice(&[text]);
    pasteboard.clearContents();
    pasteboard.writeObjects(&objects)
}

fn post_paste_shortcut() -> Result<(), TextDeliveryError> {
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|()| event_error("create paste event source"))?;
    let command_down = keyboard_event(source.clone(), COMMAND_KEY_CODE, true)?;
    let paste_down = keyboard_event(source.clone(), V_KEY_CODE, true)?;
    let paste_up = keyboard_event(source.clone(), V_KEY_CODE, false)?;
    let command_up = keyboard_event(source, COMMAND_KEY_CODE, false)?;

    command_down.set_flags(CGEventFlags::CGEventFlagCommand);
    paste_down.set_flags(CGEventFlags::CGEventFlagCommand);
    paste_up.set_flags(CGEventFlags::CGEventFlagCommand);

    for event in [command_down, paste_down, paste_up, command_up] {
        event.post(CGEventTapLocation::HID);
    }
    Ok(())
}

fn keyboard_event(
    source: CGEventSource,
    key_code: u16,
    key_down: bool,
) -> Result<CGEvent, TextDeliveryError> {
    CGEvent::new_keyboard_event(source, key_code, key_down)
        .map_err(|()| event_error("create paste keyboard event"))
}

fn event_error(operation: &str) -> TextDeliveryError {
    TextDeliveryError::System(format!("{operation} failed"))
}

struct PasteboardSnapshot(Vec<Vec<(Retained<NSString>, Retained<NSData>)>>);

impl PasteboardSnapshot {
    fn capture(pasteboard: &NSPasteboard) -> Self {
        let items = pasteboard
            .pasteboardItems()
            .map(|items| items.to_vec())
            .unwrap_or_default();
        let snapshot = items
            .iter()
            .map(|item| {
                item.types()
                    .to_vec()
                    .into_iter()
                    .filter_map(|pasteboard_type| {
                        item.dataForType(&pasteboard_type)
                            .map(|data| (pasteboard_type, data))
                    })
                    .collect()
            })
            .collect();
        Self(snapshot)
    }

    fn restore(self, pasteboard: &NSPasteboard) -> Result<(), TextDeliveryError> {
        let items: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> = self
            .0
            .into_iter()
            .map(|fields| {
                let item = NSPasteboardItem::new();
                for (pasteboard_type, data) in fields {
                    item.setData_forType(&data, &pasteboard_type);
                }
                ProtocolObject::from_retained(item)
            })
            .collect();
        let objects = NSArray::from_retained_slice(&items);
        pasteboard.clearContents();
        if items.is_empty() || pasteboard.writeObjects(&objects) {
            Ok(())
        } else {
            Err(TextDeliveryError::System(
                "failed to restore pasteboard contents".to_owned(),
            ))
        }
    }
}

impl Drop for OwnedAxElement {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0.cast()) };
    }
}

fn system_error(operation: &str, error: i32) -> TextDeliveryError {
    TextDeliveryError::System(format!("{operation}: {} ({error})", error_string(error)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_accessibility_trust_to_authorization() {
        assert_eq!(
            [
                AccessibilityAuthorization::Granted,
                AccessibilityAuthorization::Denied,
            ],
            [authorization_from(true), authorization_from(false)]
        );
    }

    #[test]
    fn verifies_collapsed_cursor_after_unicode_insertion() {
        assert!(insertion_range_matches(
            TextRange {
                location: 3,
                length: 0,
            },
            TextRange {
                location: 6,
                length: 0,
            },
            "A😀"
        ));
    }

    #[test]
    fn verifies_inserted_text_when_control_keeps_it_selected() {
        assert!(insertion_range_matches(
            TextRange {
                location: 3,
                length: 5,
            },
            TextRange {
                location: 3,
                length: 3,
            },
            "A😀"
        ));
    }

    #[test]
    fn rejects_unchanged_or_unrelated_cursor_ranges() {
        let initial = TextRange {
            location: 3,
            length: 0,
        };

        assert!(!insertion_range_matches(initial, initial, "测试"));
        assert!(!insertion_range_matches(
            initial,
            TextRange {
                location: 4,
                length: 0,
            },
            "测试"
        ));
    }
}
