use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessibilityAuthorization {
    Granted,
    Denied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextDeliveryOutcome {
    AccessibilityVerified,
    ClipboardVerified,
    ClipboardAttempted,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TextDeliveryError {
    #[error("accessibility permission is required")]
    PermissionDenied,
    #[error("no focused editable control was found")]
    NoFocusedControl,
    #[error("secure text controls do not accept dictation")]
    SecureInput,
    #[error("the focused control does not support direct text insertion")]
    UnsupportedControl,
    #[error("accessibility accepted the text but the result could not be verified")]
    AccessibilityUnverified,
    #[error("macOS accessibility operation failed: {0}")]
    System(String),
}

/// Writes final plain text into the operating system's focused editable control.
///
/// Implementations must resolve the target at delivery time, reject secure input
/// controls, report whether delivery was verified, and restore the user's clipboard
/// after any fallback paste operation. Clipboard snapshots are transient operation
/// state and must not be persisted as transcript history.
pub trait TextDeliverer: Send + Sync {
    fn authorization(&self) -> AccessibilityAuthorization;

    fn request_authorization(&self) -> AccessibilityAuthorization;

    fn deliver(&self, text: &str) -> Result<TextDeliveryOutcome, TextDeliveryError>;
}
