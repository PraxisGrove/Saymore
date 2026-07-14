use std::{ffi::c_void, ptr, thread, time::Duration, time::Instant};

use accessibility_sys::{
    AXIsProcessTrusted, AXIsProcessTrustedWithOptions, AXUIElementCopyAttributeValue,
    AXUIElementCopyParameterizedAttributeValue, AXUIElementCreateApplication,
    AXUIElementCreateSystemWide, AXUIElementGetPid, AXUIElementIsAttributeSettable, AXUIElementRef,
    AXUIElementSetAttributeValue, AXValueCreate, AXValueGetType, AXValueGetTypeID, AXValueGetValue,
    AXValueRef, error_string, kAXErrorAttributeUnsupported, kAXErrorNoValue,
    kAXErrorParameterizedAttributeUnsupported, kAXErrorSuccess, kAXFocusedUIElementAttribute,
    kAXSecureTextFieldSubrole, kAXSelectedTextAttribute, kAXSelectedTextRangeAttribute,
    kAXStringForRangeParameterizedAttribute, kAXSubroleAttribute, kAXTrustedCheckOptionPrompt,
    kAXValueTypeCFRange,
};
use core_foundation::{
    base::TCFType, boolean::CFBoolean, dictionary::CFDictionary, string::CFString,
};
use core_foundation_sys::{
    base::{CFGetTypeID, CFRange, CFRelease, CFTypeRef},
    string::{CFStringGetTypeID, CFStringRef},
};
use objc2_app_kit::{NSRunningApplication, NSWorkspace};
use template_app::{
    AccessibilityAuthorization, DeliveryTargetPrivacy, TextDeliverer, TextDeliveryError,
    TextDeliveryOutcome,
};

mod clipboard;
mod keyboard;

use clipboard::TemporaryPasteboard;

const VERIFICATION_POLL_INTERVAL: Duration = Duration::from_millis(20);
const ACCESSIBILITY_VERIFICATION_TIMEOUT: Duration = Duration::from_millis(180);
const PASTE_VERIFICATION_TIMEOUT: Duration = Duration::from_millis(700);
// Some targets consume posted paste events asynchronously, so keep both paths equally patient.
const UNOBSERVABLE_PASTE_DELAY: Duration = Duration::from_millis(700);

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    #[link_name = "IsSecureEventInputEnabled"]
    fn is_secure_event_input_enabled() -> u8;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MacOsTextDeliverer;

/// Replaces the macOS clipboard with a transcript the user explicitly chose
/// to copy from the recovery overlay.
pub fn copy_text_to_clipboard(text: &str) -> Result<(), TextDeliveryError> {
    clipboard::copy_text(text)
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

    fn target_privacy(&self) -> DeliveryTargetPrivacy {
        let secure_input = secure_event_input_enabled();
        let target = current_delivery_target();
        let subrole = target.focused.as_ref().map(|focused| {
            match focused.attribute_string(kAXSubroleAttribute) {
                Ok(Some(value)) if value == kAXSecureTextFieldSubrole => SecureSubrole::Secure,
                Ok(Some(_)) => SecureSubrole::Standard,
                Ok(None) => SecureSubrole::Unknown,
                Err(_) => SecureSubrole::Unknown,
            }
        });
        delivery_target_privacy(
            DeliveryTargetState {
                external_target: target.external_target,
                secure_input,
                focused_control: target.focused.is_some(),
            },
            subrole,
        )
    }

    fn deliver(&self, text: &str) -> Result<TextDeliveryOutcome, TextDeliveryError> {
        let secure_input = secure_event_input_enabled();
        if self.authorization() == AccessibilityAuthorization::Denied && !secure_input {
            return Err(TextDeliveryError::PermissionDenied);
        }

        let target = current_delivery_target();
        let focused = target.focused;

        match delivery_target_action(DeliveryTargetState {
            external_target: target.external_target,
            secure_input,
            focused_control: focused.is_some(),
        }) {
            DeliveryTargetAction::PasteWithoutVerification => {
                return paste_with_clipboard(PasteVerification::Unobservable, text);
            }
            DeliveryTargetAction::PasteSecurely => {
                return paste_securely(text);
            }
            DeliveryTargetAction::RejectNoTarget => {
                return if secure_input {
                    Err(TextDeliveryError::SecureDeliveryFailed(
                        "no external delivery target was found".to_owned(),
                    ))
                } else {
                    Err(TextDeliveryError::NoFocusedControl)
                };
            }
            DeliveryTargetAction::UseFocusedControl => {}
        }

        let Some(focused) = focused else {
            return Err(TextDeliveryError::NoFocusedControl);
        };

        match focused.attribute_string(kAXSubroleAttribute) {
            Ok(Some(subrole)) if subrole == kAXSecureTextFieldSubrole => {
                return paste_securely(text);
            }
            Err(_) | Ok(None) => return paste_securely(text),
            Ok(Some(_)) => {}
        }

        let initial_range = focused.selected_text_range().ok().flatten();
        let Some(initial_range) = initial_range else {
            return paste_with_clipboard(PasteVerification::Unobservable, text);
        };

        match focused.replace_selection(text) {
            Ok(()) => match verify_insertion(
                &focused,
                initial_range,
                text,
                ACCESSIBILITY_VERIFICATION_TIMEOUT,
            ) {
                InsertionVerification::Verified => Ok(TextDeliveryOutcome::AccessibilityVerified),
                InsertionVerification::Unchanged => paste_with_clipboard(
                    PasteVerification::Observable {
                        focused: &focused,
                        initial_range,
                    },
                    text,
                ),
                InsertionVerification::Unverified => {
                    Err(TextDeliveryError::AccessibilityUnverified)
                }
            },
            Err(TextDeliveryError::UnsupportedControl | TextDeliveryError::System(_)) => {
                paste_with_clipboard(
                    PasteVerification::Observable {
                        focused: &focused,
                        initial_range,
                    },
                    text,
                )
            }
            Err(error) => Err(error),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SecureSubrole {
    Standard,
    Secure,
    Unknown,
}

fn delivery_target_privacy(
    state: DeliveryTargetState,
    subrole: Option<SecureSubrole>,
) -> DeliveryTargetPrivacy {
    let unresolved_external_target = state.external_target
        && (!state.focused_control
            || !matches!(
                subrole,
                Some(SecureSubrole::Standard | SecureSubrole::Secure)
            ));
    if state.secure_input
        || unresolved_external_target
        || matches!(
            subrole,
            Some(SecureSubrole::Secure | SecureSubrole::Unknown)
        )
    {
        DeliveryTargetPrivacy::Sensitive
    } else {
        DeliveryTargetPrivacy::Standard
    }
}

struct DeliveryTarget {
    external_target: bool,
    focused: Option<OwnedAxElement>,
}

fn current_delivery_target() -> DeliveryTarget {
    let current = NSRunningApplication::currentApplication();
    let current_pid = current.processIdentifier();
    let system_focused = OwnedAxElement::system_wide()
        .and_then(|system| system.focused_control())
        .ok();
    let system_focused_pid = system_focused
        .as_ref()
        .and_then(|focused| focused.process_id().ok());
    let frontmost = NSWorkspace::sharedWorkspace().frontmostApplication();
    let frontmost_pid = frontmost
        .map(|application| application.processIdentifier())
        .filter(|process_id| *process_id != current_pid);

    match focus_resolution_action(FocusSnapshot {
        current_process: current_pid,
        system_focused_process: system_focused_pid,
        frontmost_external_process: frontmost_pid,
    }) {
        FocusResolutionAction::UseSystemFocus => DeliveryTarget {
            external_target: true,
            focused: system_focused,
        },
        FocusResolutionAction::QueryFrontmostApplication => DeliveryTarget {
            external_target: true,
            focused: frontmost_pid
                .and_then(|process_id| OwnedAxElement::application(process_id).ok())
                .and_then(|application| application.focused_control().ok()),
        },
        FocusResolutionAction::RejectNoTarget => DeliveryTarget {
            external_target: false,
            focused: None,
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusResolutionAction {
    UseSystemFocus,
    QueryFrontmostApplication,
    RejectNoTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FocusSnapshot {
    current_process: i32,
    system_focused_process: Option<i32>,
    frontmost_external_process: Option<i32>,
}

fn focus_resolution_action(snapshot: FocusSnapshot) -> FocusResolutionAction {
    match snapshot.system_focused_process {
        Some(process_id) if process_id != snapshot.current_process => {
            FocusResolutionAction::UseSystemFocus
        }
        Some(_) if snapshot.frontmost_external_process.is_some() => {
            FocusResolutionAction::QueryFrontmostApplication
        }
        Some(_) => FocusResolutionAction::RejectNoTarget,
        None if snapshot.frontmost_external_process.is_some() => {
            FocusResolutionAction::QueryFrontmostApplication
        }
        None => FocusResolutionAction::RejectNoTarget,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeliveryTargetAction {
    UseFocusedControl,
    PasteWithoutVerification,
    PasteSecurely,
    RejectNoTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DeliveryTargetState {
    external_target: bool,
    secure_input: bool,
    focused_control: bool,
}

fn delivery_target_action(state: DeliveryTargetState) -> DeliveryTargetAction {
    match (
        state.external_target,
        state.secure_input,
        state.focused_control,
    ) {
        (false, _, _) => DeliveryTargetAction::RejectNoTarget,
        (true, true, _) => DeliveryTargetAction::PasteSecurely,
        (true, false, true) => DeliveryTargetAction::UseFocusedControl,
        (true, false, false) => DeliveryTargetAction::PasteWithoutVerification,
    }
}

fn secure_event_input_enabled() -> bool {
    // SAFETY: IsSecureEventInputEnabled reads process-global system state and has no preconditions.
    unsafe { is_secure_event_input_enabled() != 0 }
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

    fn application(process_id: i32) -> Result<Self, TextDeliveryError> {
        let element = unsafe { AXUIElementCreateApplication(process_id) };
        if element.is_null() {
            Err(TextDeliveryError::System(
                "AXUIElementCreateApplication returned null".to_owned(),
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

    fn process_id(&self) -> Result<i32, TextDeliveryError> {
        let mut process_id = 0;
        let error = unsafe { AXUIElementGetPid(self.0, &mut process_id) };
        if error == kAXErrorSuccess {
            Ok(process_id)
        } else {
            Err(system_error("read focused control process", error))
        }
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

    fn string_for_range(&self, range: TextRange) -> Result<Option<String>, TextDeliveryError> {
        let Some(range) = range.to_cf_range() else {
            return Err(TextDeliveryError::System(
                "text verification range exceeds macOS limits".to_owned(),
            ));
        };
        let parameter =
            unsafe { AXValueCreate(kAXValueTypeCFRange, ptr::from_ref(&range).cast::<c_void>()) };
        if parameter.is_null() {
            return Err(TextDeliveryError::System(
                "AXValueCreate returned null for text verification range".to_owned(),
            ));
        }

        let attribute = CFString::new(kAXStringForRangeParameterizedAttribute);
        let mut value: CFTypeRef = ptr::null();
        let error = unsafe {
            AXUIElementCopyParameterizedAttributeValue(
                self.0,
                attribute.as_concrete_TypeRef(),
                parameter.cast(),
                &mut value,
            )
        };
        unsafe { CFRelease(parameter.cast()) };

        if error == kAXErrorParameterizedAttributeUnsupported
            || error == kAXErrorAttributeUnsupported
            || error == kAXErrorNoValue
            || value.is_null()
        {
            return Ok(None);
        }
        if error != kAXErrorSuccess {
            return Err(system_error("read inserted text", error));
        }
        if unsafe { CFGetTypeID(value) } != unsafe { CFStringGetTypeID() } {
            unsafe { CFRelease(value) };
            return Ok(None);
        }

        let value = unsafe { CFString::wrap_under_create_rule(value.cast::<_>() as CFStringRef) };
        Ok(Some(value.to_string()))
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

    fn to_cf_range(self) -> Option<CFRange> {
        Some(CFRange::init(
            isize::try_from(self.location).ok()?,
            isize::try_from(self.length).ok()?,
        ))
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
                let inserted_range = TextRange {
                    location: initial.location,
                    length: text.encode_utf16().count(),
                };
                return match focused.string_for_range(inserted_range) {
                    Ok(observed_text) => {
                        verify_observed_insertion(initial, current, text, observed_text.as_deref())
                    }
                    Err(_) => InsertionVerification::Unverified,
                };
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

fn verify_observed_insertion(
    initial: TextRange,
    current: TextRange,
    expected_text: &str,
    observed_text: Option<&str>,
) -> InsertionVerification {
    if !insertion_range_matches(initial, current, expected_text) {
        return InsertionVerification::Unchanged;
    }
    match observed_text {
        Some(observed_text) if observed_text == expected_text => InsertionVerification::Verified,
        Some(_) => InsertionVerification::Unverified,
        None => InsertionVerification::Verified,
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

enum PasteVerification<'a> {
    Observable {
        focused: &'a OwnedAxElement,
        initial_range: TextRange,
    },
    Unobservable,
    Secure,
}

fn paste_with_clipboard(
    verification: PasteVerification<'_>,
    text: &str,
) -> Result<TextDeliveryOutcome, TextDeliveryError> {
    let temporary = TemporaryPasteboard::general(text)?;
    if let Err(error) = keyboard::post_paste_shortcut() {
        temporary.restore_if_unchanged()?;
        return Err(error);
    }

    let outcome = match verification {
        PasteVerification::Observable {
            focused,
            initial_range,
        } => match verify_insertion(focused, initial_range, text, PASTE_VERIFICATION_TIMEOUT) {
            InsertionVerification::Verified => Ok(TextDeliveryOutcome::ClipboardVerified),
            InsertionVerification::Unchanged | InsertionVerification::Unverified => {
                Ok(TextDeliveryOutcome::ClipboardAttempted)
            }
        },
        PasteVerification::Unobservable => {
            thread::sleep(UNOBSERVABLE_PASTE_DELAY);
            Ok(TextDeliveryOutcome::ClipboardAttempted)
        }
        PasteVerification::Secure => {
            thread::sleep(UNOBSERVABLE_PASTE_DELAY);
            Ok(TextDeliveryOutcome::SecureClipboardAttempted)
        }
    };

    temporary.restore_if_unchanged()?;
    outcome
}

fn paste_securely(text: &str) -> Result<TextDeliveryOutcome, TextDeliveryError> {
    paste_with_clipboard(PasteVerification::Secure, text)
        .map_err(|error| TextDeliveryError::SecureDeliveryFailed(error.to_string()))
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
mod tests;
