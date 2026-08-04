use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessibilityAuthorization {
    Granted,
    Denied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryTargetPrivacy {
    Standard,
    Sensitive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextDeliveryOutcome {
    AccessibilityVerified,
    ClipboardVerified,
    /// A paste shortcut was issued, but the target did not expose a way to verify its result.
    ClipboardAttempted,
    /// A restricted paste was issued to a secure control, whose contents cannot be inspected.
    SecureClipboardAttempted,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TextDeliveryError {
    #[error("accessibility permission is required")]
    PermissionDenied,
    #[error("no focused editable control was found")]
    NoFocusedControl,
    #[error("the focused control does not support direct text insertion")]
    UnsupportedControl,
    #[error("accessibility accepted the text but the result could not be verified")]
    AccessibilityUnverified,
    /// A secure control was targeted, but its restricted paste could not be completed.
    #[error("restricted secure text delivery failed: {0}")]
    SecureDeliveryFailed(String),
    #[error("operating-system accessibility operation failed: {0}")]
    System(String),
}

/// Writes final plain text into the operating system's focused editable control.
///
/// Implementations must resolve the target at delivery time, report whether delivery
/// was verified, and restore the user's clipboard after any fallback paste operation.
/// Secure controls may receive an unverified restricted paste, but their transcript and
/// clipboard snapshots are transient operation state and must not be persisted.
///
/// Callers migrating from the former `SecureInput` rejection must handle
/// `SecureClipboardAttempted` as an unverified outcome and `SecureDeliveryFailed` as a
/// sensitive failure; neither result may cause the transcript to be persisted.
pub trait TextDeliverer: Send + Sync {
    fn authorization(&self) -> AccessibilityAuthorization;

    fn request_authorization(&self) -> AccessibilityAuthorization;

    /// Classifies the currently focused delivery target without writing text.
    ///
    /// Implementations must classify unknown secure-control metadata as sensitive.
    /// Callers use this as a privacy preflight and must still handle a target that
    /// becomes sensitive before the subsequent delivery attempt.
    fn target_privacy(&self) -> DeliveryTargetPrivacy;

    fn deliver(&self, text: &str) -> Result<TextDeliveryOutcome, TextDeliveryError>;
}

/// Delivers text and, when the target exposes a safe text range, observes local edits.
///
/// Implementations must keep native control handles and surrounding text transient, stop at
/// sensitive or unsupported controls, ignore control resets and placeholder text, and invoke the
/// observer with transient snapshots followed by exactly one end event.
pub trait CorrectionObservingTextDeliverer: TextDeliverer {
    fn deliver_and_observe(
        &self,
        text: &str,
        observer: crate::TextRevisionObserver,
    ) -> Result<TextDeliveryOutcome, TextDeliveryError>;

    /// Ends the currently active correction observation, if any.
    ///
    /// Starting a new dictation must call this before recording so the prior
    /// delivered text is finalized even when the new dictation is cancelled.
    fn finish_observation(&self, reason: crate::TextRevisionEndReason);
}
