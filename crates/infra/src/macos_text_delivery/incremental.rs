use std::{thread, time::Duration};

use template_app::{TextDeliveryError, TextDeliveryOutcome, TextEditObserver};

use super::{
    ACCESSIBILITY_VERIFICATION_TIMEOUT, AccessibilityAuthorization, DeliveryTargetAction,
    DeliveryTargetState, FOCUS_SETTLE_DELAY, InsertionVerification, OwnedAxElement,
    PASTE_VERIFICATION_TIMEOUT, TemporaryPasteboard, TextRange, UNOBSERVABLE_PASTE_DELAY,
    VERIFICATION_POLL_INTERVAL, authorization_from, current_delivery_target,
    delivery_target_action, keyboard, secure_event_input_enabled, verify_observed_insertion,
};
use crate::macos_text_delivery::observation::CorrectionObservationTarget;

pub struct MacOsTextDeliverySession {
    text: String,
    observer: Option<TextEditObserver>,
    state: Option<DeliveryState>,
}

pub enum MacOsTextDeliveryProgress {
    Wait(Duration),
    Complete(Result<TextDeliveryOutcome, TextDeliveryError>),
}

enum DeliveryState {
    ResolveTarget,
    DeliverToFocused(OwnedAxElement),
    VerifyDirect(Verification),
    BeginPaste(PasteTarget),
    VerifyPaste {
        temporary: TemporaryPasteboard,
        verification: Verification,
    },
    AwaitPaste {
        temporary: TemporaryPasteboard,
        outcome: TextDeliveryOutcome,
        secure: bool,
    },
}

struct Verification {
    focused: OwnedAxElement,
    initial_range: TextRange,
    remaining: Duration,
    range_changed: bool,
}

enum PasteTarget {
    Observable {
        focused: OwnedAxElement,
        initial_range: TextRange,
    },
    Unobservable,
    Secure,
}

struct DeliveryAttempt {
    outcome: TextDeliveryOutcome,
    observation: Option<CorrectionObservationTarget>,
}

enum VerificationPoll {
    Complete(InsertionVerification),
    Pending { range_changed: bool },
}

impl MacOsTextDeliverySession {
    pub(super) fn new(text: String, observer: Option<TextEditObserver>) -> Self {
        Self {
            text,
            observer,
            state: Some(DeliveryState::ResolveTarget),
        }
    }

    pub fn initial_delay() -> Duration {
        FOCUS_SETTLE_DELAY
    }

    pub fn advance(&mut self) -> MacOsTextDeliveryProgress {
        let Some(state) = self.state.take() else {
            return MacOsTextDeliveryProgress::Complete(Err(TextDeliveryError::System(
                "macOS text delivery session was already completed".to_owned(),
            )));
        };
        match state {
            DeliveryState::ResolveTarget => self.resolve_target(),
            DeliveryState::DeliverToFocused(focused) => self.deliver_to_focused(focused),
            DeliveryState::VerifyDirect(verification) => self.verify_direct(verification),
            DeliveryState::BeginPaste(target) => self.begin_paste(target),
            DeliveryState::VerifyPaste {
                temporary,
                verification,
            } => self.verify_paste(temporary, verification),
            DeliveryState::AwaitPaste {
                temporary,
                outcome,
                secure,
            } => self.finish_paste(temporary, outcome, secure, None),
        }
    }

    fn resolve_target(&mut self) -> MacOsTextDeliveryProgress {
        let secure_input = secure_event_input_enabled();
        if authorization_from(unsafe { accessibility_sys::AXIsProcessTrusted() })
            == AccessibilityAuthorization::Denied
            && !secure_input
        {
            return self.complete(Err(TextDeliveryError::PermissionDenied));
        }

        let target = current_delivery_target();
        let focused = target.focused;
        match delivery_target_action(DeliveryTargetState {
            external_target: target.external_target,
            secure_input,
            focused_control: focused.is_some(),
        }) {
            DeliveryTargetAction::UseFocusedControl => match focused {
                Some(focused) => {
                    self.wait_for(DeliveryState::DeliverToFocused(focused), Duration::ZERO)
                }
                None => self.complete(Err(TextDeliveryError::NoFocusedControl)),
            },
            DeliveryTargetAction::PasteWithoutVerification => self.wait_for(
                DeliveryState::BeginPaste(PasteTarget::Unobservable),
                Duration::ZERO,
            ),
            DeliveryTargetAction::PasteSecurely => self.wait_for(
                DeliveryState::BeginPaste(PasteTarget::Secure),
                Duration::ZERO,
            ),
            DeliveryTargetAction::RejectNoTarget if secure_input => {
                self.complete(Err(TextDeliveryError::SecureDeliveryFailed(
                    "no external delivery target was found".to_owned(),
                )))
            }
            DeliveryTargetAction::RejectNoTarget => {
                self.complete(Err(TextDeliveryError::NoFocusedControl))
            }
        }
    }

    fn deliver_to_focused(&mut self, focused: OwnedAxElement) -> MacOsTextDeliveryProgress {
        if matches!(
            focused.attribute_string(accessibility_sys::kAXSubroleAttribute),
            Ok(Some(subrole)) if subrole == accessibility_sys::kAXSecureTextFieldSubrole
        ) {
            return self.wait_for(
                DeliveryState::BeginPaste(PasteTarget::Secure),
                Duration::ZERO,
            );
        }

        let Some(initial_range) = focused.selected_text_range().ok().flatten() else {
            return self.wait_for(
                DeliveryState::BeginPaste(PasteTarget::Unobservable),
                Duration::ZERO,
            );
        };
        match focused.replace_selection(&self.text) {
            Ok(()) => self.wait_for(
                DeliveryState::VerifyDirect(Verification {
                    focused,
                    initial_range,
                    remaining: ACCESSIBILITY_VERIFICATION_TIMEOUT,
                    range_changed: false,
                }),
                Duration::ZERO,
            ),
            Err(TextDeliveryError::UnsupportedControl | TextDeliveryError::System(_)) => self
                .wait_for(
                    DeliveryState::BeginPaste(PasteTarget::Observable {
                        focused,
                        initial_range,
                    }),
                    Duration::ZERO,
                ),
            Err(error) => self.complete(Err(error)),
        }
    }

    fn verify_direct(&mut self, mut verification: Verification) -> MacOsTextDeliveryProgress {
        match poll_verification(
            &verification.focused,
            verification.initial_range,
            &self.text,
        ) {
            VerificationPoll::Complete(InsertionVerification::Verified) => {
                let observation = CorrectionObservationTarget::capture(
                    verification.focused,
                    verification.initial_range,
                    &self.text,
                );
                self.complete(Ok(DeliveryAttempt {
                    outcome: TextDeliveryOutcome::AccessibilityVerified,
                    observation,
                }))
            }
            VerificationPoll::Complete(InsertionVerification::Unverified) => {
                self.complete(Err(TextDeliveryError::AccessibilityUnverified))
            }
            VerificationPoll::Complete(InsertionVerification::Unchanged) => self.wait_for(
                DeliveryState::BeginPaste(PasteTarget::Observable {
                    focused: verification.focused,
                    initial_range: verification.initial_range,
                }),
                Duration::ZERO,
            ),
            VerificationPoll::Pending { range_changed } => {
                verification.range_changed |= range_changed;
                if verification.remaining.is_zero() {
                    if verification.range_changed {
                        self.complete(Err(TextDeliveryError::AccessibilityUnverified))
                    } else {
                        self.wait_for(
                            DeliveryState::BeginPaste(PasteTarget::Observable {
                                focused: verification.focused,
                                initial_range: verification.initial_range,
                            }),
                            Duration::ZERO,
                        )
                    }
                } else {
                    self.wait_for_verification(DeliveryState::VerifyDirect, verification)
                }
            }
        }
    }

    fn begin_paste(&mut self, target: PasteTarget) -> MacOsTextDeliveryProgress {
        let secure = matches!(target, PasteTarget::Secure);
        let temporary = match TemporaryPasteboard::general(&self.text) {
            Ok(temporary) => temporary,
            Err(error) => return self.complete(Err(map_secure_error(secure, error))),
        };
        if let Err(error) = keyboard::post_paste_shortcut() {
            let result = temporary
                .restore_if_unchanged()
                .and(Err(error))
                .map_err(|error| map_secure_error(secure, error));
            return self.complete(result);
        }

        match target {
            PasteTarget::Observable {
                focused,
                initial_range,
            } => self.wait_for(
                DeliveryState::VerifyPaste {
                    temporary,
                    verification: Verification {
                        focused,
                        initial_range,
                        remaining: PASTE_VERIFICATION_TIMEOUT,
                        range_changed: false,
                    },
                },
                Duration::ZERO,
            ),
            PasteTarget::Unobservable => self.wait_for(
                DeliveryState::AwaitPaste {
                    temporary,
                    outcome: TextDeliveryOutcome::ClipboardAttempted,
                    secure: false,
                },
                UNOBSERVABLE_PASTE_DELAY,
            ),
            PasteTarget::Secure => self.wait_for(
                DeliveryState::AwaitPaste {
                    temporary,
                    outcome: TextDeliveryOutcome::SecureClipboardAttempted,
                    secure: true,
                },
                UNOBSERVABLE_PASTE_DELAY,
            ),
        }
    }

    fn verify_paste(
        &mut self,
        temporary: TemporaryPasteboard,
        mut verification: Verification,
    ) -> MacOsTextDeliveryProgress {
        match poll_verification(
            &verification.focused,
            verification.initial_range,
            &self.text,
        ) {
            VerificationPoll::Complete(InsertionVerification::Verified) => {
                let observation = CorrectionObservationTarget::capture(
                    verification.focused,
                    verification.initial_range,
                    &self.text,
                );
                self.finish_paste(
                    temporary,
                    TextDeliveryOutcome::ClipboardVerified,
                    false,
                    observation,
                )
            }
            VerificationPoll::Complete(
                InsertionVerification::Unchanged | InsertionVerification::Unverified,
            ) => self.finish_paste(
                temporary,
                TextDeliveryOutcome::ClipboardAttempted,
                false,
                None,
            ),
            VerificationPoll::Pending { range_changed } => {
                verification.range_changed |= range_changed;
                if verification.remaining.is_zero() {
                    self.finish_paste(
                        temporary,
                        TextDeliveryOutcome::ClipboardAttempted,
                        false,
                        None,
                    )
                } else {
                    let delay = next_verification_delay(&mut verification);
                    self.wait_for(
                        DeliveryState::VerifyPaste {
                            temporary,
                            verification,
                        },
                        delay,
                    )
                }
            }
        }
    }

    fn finish_paste(
        &mut self,
        temporary: TemporaryPasteboard,
        outcome: TextDeliveryOutcome,
        secure: bool,
        observation: Option<CorrectionObservationTarget>,
    ) -> MacOsTextDeliveryProgress {
        match temporary.restore_if_unchanged() {
            Ok(()) => self.complete(Ok(DeliveryAttempt {
                outcome,
                observation,
            })),
            Err(error) => self.complete(Err(map_secure_error(secure, error))),
        }
    }

    fn wait_for_verification(
        &mut self,
        state: impl FnOnce(Verification) -> DeliveryState,
        mut verification: Verification,
    ) -> MacOsTextDeliveryProgress {
        let delay = next_verification_delay(&mut verification);
        self.wait_for(state(verification), delay)
    }

    fn wait_for(&mut self, state: DeliveryState, delay: Duration) -> MacOsTextDeliveryProgress {
        self.state = Some(state);
        MacOsTextDeliveryProgress::Wait(delay)
    }

    fn complete(
        &mut self,
        result: Result<DeliveryAttempt, TextDeliveryError>,
    ) -> MacOsTextDeliveryProgress {
        let result = result.map(|attempt| {
            if let (Some(target), Some(observer)) = (attempt.observation, self.observer.take()) {
                let _ = thread::Builder::new()
                    .name("saymore-correction-observer".to_owned())
                    .spawn(move || target.observe(observer));
            }
            attempt.outcome
        });
        MacOsTextDeliveryProgress::Complete(result)
    }
}

impl Drop for MacOsTextDeliverySession {
    fn drop(&mut self) {
        let temporary = match self.state.take() {
            Some(DeliveryState::VerifyPaste { temporary, .. })
            | Some(DeliveryState::AwaitPaste { temporary, .. }) => Some(temporary),
            _ => None,
        };
        if let Some(temporary) = temporary {
            let _ = temporary.restore_if_unchanged();
        }
    }
}

fn poll_verification(focused: &OwnedAxElement, initial: TextRange, text: &str) -> VerificationPoll {
    match focused.selected_text_range() {
        Ok(Some(current)) if super::insertion_range_matches(initial, current, text) => {
            let inserted_range = TextRange {
                location: initial.location,
                length: text.encode_utf16().count(),
            };
            let verification = match focused.string_for_range(inserted_range) {
                Ok(observed_text) => {
                    verify_observed_insertion(initial, current, text, observed_text.as_deref())
                }
                Err(_) => InsertionVerification::Unverified,
            };
            VerificationPoll::Complete(verification)
        }
        Ok(Some(current)) => VerificationPoll::Pending {
            range_changed: current != initial,
        },
        Ok(None) | Err(_) => VerificationPoll::Complete(InsertionVerification::Unverified),
    }
}

fn next_verification_delay(verification: &mut Verification) -> Duration {
    let delay = verification.remaining.min(VERIFICATION_POLL_INTERVAL);
    verification.remaining = verification.remaining.saturating_sub(delay);
    delay
}

fn map_secure_error(secure: bool, error: TextDeliveryError) -> TextDeliveryError {
    if secure {
        TextDeliveryError::SecureDeliveryFailed(error.to_string())
    } else {
        error
    }
}

pub(super) fn run_synchronously(
    text: String,
    observer: Option<TextEditObserver>,
) -> Result<TextDeliveryOutcome, TextDeliveryError> {
    let mut session = MacOsTextDeliverySession::new(text, observer);
    thread::sleep(MacOsTextDeliverySession::initial_delay());
    loop {
        match session.advance() {
            MacOsTextDeliveryProgress::Wait(delay) => thread::sleep(delay),
            MacOsTextDeliveryProgress::Complete(result) => return result,
        }
    }
}
