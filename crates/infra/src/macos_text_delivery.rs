use std::{ffi::c_void, io, process::Command, ptr, time::Duration};

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
    number::{
        CFBooleanGetTypeID, CFBooleanGetValue, CFBooleanRef, CFNumberGetTypeID, CFNumberGetValue,
        CFNumberRef, kCFNumberSInt64Type,
    },
    string::{CFStringGetTypeID, CFStringRef},
};
use objc2_app_kit::{NSRunningApplication, NSWorkspace};
use template_app::{
    AccessibilityAuthorization, CorrectionObservingTextDeliverer, DeliveryTargetPrivacy,
    TextDeliverer, TextDeliveryError, TextDeliveryOutcome, TextEditObserver,
};

mod ax;
mod capabilities;
mod clipboard;
mod incremental;
mod keyboard;
mod observation;

use ax::OwnedAxElement;
pub use capabilities::{
    MacOsCorrectionObservationSupport, MacOsFocusedTextControlCapabilities,
    focused_text_control_capabilities, text_control_capabilities_for_process,
};
use clipboard::TemporaryPasteboard;
pub use incremental::{MacOsTextDeliveryProgress, MacOsTextDeliverySession};
#[cfg(test)]
use observation::text_between_anchors;

const VERIFICATION_POLL_INTERVAL: Duration = Duration::from_millis(20);
const ACCESSIBILITY_VERIFICATION_TIMEOUT: Duration = Duration::from_millis(180);
const PASTE_VERIFICATION_TIMEOUT: Duration = Duration::from_millis(700);
const FOCUS_SETTLE_DELAY: Duration = Duration::from_millis(80);
// Some targets consume posted paste events asynchronously, so keep both paths equally patient.
const UNOBSERVABLE_PASTE_DELAY: Duration = Duration::from_millis(700);

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    #[link_name = "IsSecureEventInputEnabled"]
    fn is_secure_event_input_enabled() -> u8;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MacOsTextDeliverer;

impl MacOsTextDeliverer {
    pub fn begin_delivery(text: String) -> MacOsTextDeliverySession {
        MacOsTextDeliverySession::new(text, None)
    }

    pub fn begin_delivery_and_observe(
        text: String,
        observer: TextEditObserver,
    ) -> MacOsTextDeliverySession {
        MacOsTextDeliverySession::new(text, Some(observer))
    }
}

/// Replaces the macOS clipboard with a transcript the user explicitly chose
/// to copy from the recovery overlay.
pub fn copy_text_to_clipboard(text: &str) -> Result<(), TextDeliveryError> {
    clipboard::copy_text(text)
}

/// Opens the macOS Accessibility privacy pane for UI callers handling missing permission.
pub fn open_accessibility_privacy_settings() -> Result<(), io::Error> {
    let status = Command::new("/usr/bin/open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "System Settings exited with status {status}"
        )))
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
        incremental::run_synchronously(text.to_owned(), None)
    }
}

impl CorrectionObservingTextDeliverer for MacOsTextDeliverer {
    fn deliver_and_observe(
        &self,
        text: &str,
        observer: TextEditObserver,
    ) -> Result<TextDeliveryOutcome, TextDeliveryError> {
        incremental::run_synchronously(text.to_owned(), Some(observer))
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
    if state.secure_input || matches!(subrole, Some(SecureSubrole::Secure)) {
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
        FocusResolutionAction::QueryFrontmostApplication => {
            let focused = frontmost_pid
                .and_then(|process_id| OwnedAxElement::application(process_id).ok())
                .and_then(|application| application.focused_control().ok());
            DeliveryTarget {
                external_target: true,
                focused,
            }
        }
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
