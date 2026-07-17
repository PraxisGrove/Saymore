use slint::{ComponentHandle, SharedString};
use template_app::{
    AccessibilityAuthorization, MicrophoneAuthorization, ProcessedText, RecordingError,
    RefinementStatus, SpeechRecognitionError, TextDeliveryError, TextDeliveryOutcome,
};

use crate::ui::{AppWindow, Translations};

pub fn apply_recording_started(ui: &AppWindow) {
    ui.set_recording_active(true);
    ui.set_recording_failed(false);
    ui.set_recording_complete(false);
    ui.set_recording_attempted(false);
    ui.set_recording_level(0.0);
    let translations = ui.global::<Translations>();
    ui.set_recording_status(translations.get_recording_active());
    ui.set_recording_detail(translations.invoke_recording_samples(format_duration(0).into(), 0));
}

pub fn apply_transcription_completed(
    ui: &AppWindow,
    audio_duration_ms: u64,
    processed: &ProcessedText,
    delivery: Result<TextDeliveryOutcome, TextDeliveryError>,
) {
    let secure = delivery_is_silent(&delivery);
    let verified = matches!(
        &delivery,
        Ok(TextDeliveryOutcome::AccessibilityVerified | TextDeliveryOutcome::ClipboardVerified)
    );
    let attempted = matches!(&delivery, Ok(TextDeliveryOutcome::ClipboardAttempted));
    ui.set_recording_active(false);
    if secure {
        ui.set_recording_complete(false);
        ui.set_recording_attempted(false);
        ui.set_recording_failed(false);
        ui.set_recording_level(0.0);
        return;
    }
    ui.set_recording_complete(verified);
    ui.set_recording_attempted(attempted);
    ui.set_recording_failed(delivery.is_err());
    ui.set_recording_level(0.0);
    match delivery {
        Ok(outcome) => {
            ui.set_recording_status(delivery_status(ui, &processed.refinement, outcome));
            ui.set_recording_detail(completion_detail(ui, audio_duration_ms, processed, outcome));
        }
        Err(error) => {
            let translations = ui.global::<Translations>();
            ui.set_recording_status(translations.get_recording_delivery_failed());
            ui.set_recording_detail(text_delivery_error_message(ui, &error));
        }
    }
}

fn delivery_is_silent(delivery: &Result<TextDeliveryOutcome, TextDeliveryError>) -> bool {
    matches!(
        delivery,
        Ok(TextDeliveryOutcome::SecureClipboardAttempted)
            | Err(TextDeliveryError::SecureDeliveryFailed(_))
    )
}

fn completion_status(ui: &AppWindow, refinement: &RefinementStatus) -> SharedString {
    let translations = ui.global::<Translations>();
    match refinement {
        RefinementStatus::Disabled | RefinementStatus::Skipped(_) => {
            translations.get_recording_transcription_complete()
        }
        RefinementStatus::Completed => translations.get_recording_polishing_complete(),
        RefinementStatus::FellBack(_) => translations.get_recording_polishing_incomplete(),
    }
}

fn delivery_status(
    ui: &AppWindow,
    refinement: &RefinementStatus,
    outcome: TextDeliveryOutcome,
) -> SharedString {
    let translations = ui.global::<Translations>();
    match outcome {
        TextDeliveryOutcome::AccessibilityVerified | TextDeliveryOutcome::ClipboardVerified => {
            completion_status(ui, refinement)
        }
        TextDeliveryOutcome::ClipboardAttempted => translations.get_recording_delivery_attempted(),
        TextDeliveryOutcome::SecureClipboardAttempted => {
            translations.get_recording_secure_input_attempted()
        }
    }
}

fn completion_detail(
    ui: &AppWindow,
    audio_duration_ms: u64,
    processed: &ProcessedText,
    outcome: TextDeliveryOutcome,
) -> SharedString {
    if let RefinementStatus::FellBack(reason) = &processed.refinement {
        return fallback_detail(ui, reason);
    }
    ui.global::<Translations>().invoke_recording_completion(
        format_duration(audio_duration_ms).into(),
        i32::try_from(processed.text.chars().count()).unwrap_or(i32::MAX),
        delivery_outcome_label(ui, outcome),
    )
}

pub fn fallback_detail(
    ui: &AppWindow,
    reason: &template_app::RefinementFallbackReason,
) -> SharedString {
    use template_app::RefinementFallbackReason;

    let translations = ui.global::<Translations>();
    match reason {
        RefinementFallbackReason::Timeout
        | RefinementFallbackReason::Transport
        | RefinementFallbackReason::Quota
        | RefinementFallbackReason::TemporarilyUnavailable => {
            translations.get_refinement_temporarily_unavailable()
        }
        RefinementFallbackReason::OutputRejected => translations.get_refinement_output_rejected(),
        RefinementFallbackReason::NotConfigured
        | RefinementFallbackReason::Authentication
        | RefinementFallbackReason::InvalidConfiguration
        | RefinementFallbackReason::ModelUnavailable
        | RefinementFallbackReason::Protocol => translations.get_refinement_incomplete(),
    }
}

pub fn delivery_requires_copy_recovery(
    delivery: &Result<TextDeliveryOutcome, TextDeliveryError>,
) -> bool {
    matches!(
        delivery,
        Err(TextDeliveryError::PermissionDenied
            | TextDeliveryError::NoFocusedControl
            | TextDeliveryError::UnsupportedControl
            | TextDeliveryError::AccessibilityUnverified
            | TextDeliveryError::System(_))
    )
}

pub fn apply_asr_error(ui: &AppWindow, error: &SpeechRecognitionError) {
    ui.set_recording_active(false);
    ui.set_recording_complete(false);
    ui.set_recording_attempted(false);
    ui.set_recording_failed(true);
    ui.set_recording_level(0.0);
    let translations = ui.global::<Translations>();
    ui.set_recording_status(translations.get_recording_recognition_failed());
    ui.set_recording_detail(asr_error_message(ui, error));
}

pub fn apply_recording_error(ui: &AppWindow, error: &RecordingError) {
    ui.set_recording_active(false);
    ui.set_recording_failed(true);
    ui.set_recording_complete(false);
    ui.set_recording_attempted(false);
    ui.set_recording_level(0.0);
    let translations = ui.global::<Translations>();
    ui.set_recording_status(translations.get_recording_capture_failed());
    ui.set_recording_detail(recording_error_message(ui, error));
}

pub fn format_duration(milliseconds: u64) -> String {
    format!(
        "{}.{:01}",
        milliseconds / 1_000,
        (milliseconds % 1_000) / 100
    )
}

pub fn update_authorizations(
    ui: &AppWindow,
    accessibility: AccessibilityAuthorization,
    microphone: MicrophoneAuthorization,
) {
    update_accessibility_authorization(ui, accessibility);
    update_microphone_authorization(ui, microphone);
}

pub fn update_accessibility_authorization(
    ui: &AppWindow,
    authorization: AccessibilityAuthorization,
) {
    let granted = authorization == AccessibilityAuthorization::Granted;
    ui.set_authorized(granted);
    let translations = ui.global::<Translations>();
    ui.set_authorization_status(if granted {
        translations.get_permission_authorized()
    } else {
        translations.get_permission_not_authorized()
    });
}

pub fn update_microphone_authorization(ui: &AppWindow, authorization: MicrophoneAuthorization) {
    let granted = authorization == MicrophoneAuthorization::Granted;
    ui.set_microphone_authorized(granted);
    let translations = ui.global::<Translations>();
    ui.set_microphone_status(match authorization {
        MicrophoneAuthorization::NotDetermined => translations.get_permission_not_requested(),
        MicrophoneAuthorization::Granted => translations.get_permission_authorized(),
        MicrophoneAuthorization::Denied => translations.get_permission_denied(),
        MicrophoneAuthorization::Restricted => translations.get_permission_restricted(),
    });
}

fn delivery_outcome_label(ui: &AppWindow, outcome: TextDeliveryOutcome) -> SharedString {
    let translations = ui.global::<Translations>();
    match outcome {
        TextDeliveryOutcome::AccessibilityVerified => translations.get_delivery_inserted(),
        TextDeliveryOutcome::ClipboardVerified => translations.get_delivery_pasted(),
        TextDeliveryOutcome::ClipboardAttempted => translations.get_delivery_paste_attempted(),
        TextDeliveryOutcome::SecureClipboardAttempted => {
            translations.get_recording_secure_input_attempted()
        }
    }
}

fn text_delivery_error_message(ui: &AppWindow, error: &TextDeliveryError) -> SharedString {
    let translations = ui.global::<Translations>();
    match error {
        TextDeliveryError::PermissionDenied => translations.get_delivery_accessibility_required(),
        TextDeliveryError::NoFocusedControl => translations.get_delivery_no_focused_control(),
        TextDeliveryError::UnsupportedControl => translations.get_delivery_unsupported_control(),
        TextDeliveryError::AccessibilityUnverified => translations.get_delivery_unverified(),
        TextDeliveryError::SecureDeliveryFailed(_) => translations.get_delivery_secure_failed(),
        TextDeliveryError::System(_) => translations.get_delivery_system_failed(),
    }
}

fn recording_error_message(ui: &AppWindow, error: &RecordingError) -> SharedString {
    let translations = ui.global::<Translations>();
    match error {
        RecordingError::PermissionDenied => translations.get_recording_microphone_required(),
        RecordingError::NoInputDevice => translations.get_recording_no_input_device(),
        RecordingError::AlreadyRecording => translations.get_recording_already_active(),
        RecordingError::NotRecording => translations.get_recording_not_active(),
        RecordingError::UnsupportedSampleFormat(_) => {
            translations.get_recording_unsupported_format()
        }
        RecordingError::Capture(_) => translations.get_recording_capture_error(),
    }
}

fn asr_error_message(ui: &AppWindow, error: &SpeechRecognitionError) -> SharedString {
    let translations = ui.global::<Translations>();
    match error {
        SpeechRecognitionError::NotConfigured => translations.get_asr_not_configured(),
        SpeechRecognitionError::Authentication => translations.get_asr_authentication(),
        SpeechRecognitionError::Quota => translations.get_asr_quota(),
        SpeechRecognitionError::Transport(_) => translations.get_asr_transport(),
        SpeechRecognitionError::Protocol(_) => translations.get_asr_protocol(),
        SpeechRecognitionError::Timeout => translations.get_asr_timeout(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_recording_duration_to_tenths() {
        assert_eq!("0.0", format_duration(0));
        assert_eq!("1.2", format_duration(1_250));
        assert_eq!("61.0", format_duration(61_099));
    }

    #[test]
    fn issued_clipboard_paste_does_not_open_copy_recovery() {
        assert!(!delivery_requires_copy_recovery(&Ok(
            TextDeliveryOutcome::ClipboardAttempted
        )));
        assert!(!delivery_requires_copy_recovery(&Ok(
            TextDeliveryOutcome::SecureClipboardAttempted
        )));
        assert!(!delivery_requires_copy_recovery(&Err(
            TextDeliveryError::SecureDeliveryFailed("restricted".to_owned())
        )));
        assert!(delivery_is_silent(&Ok(
            TextDeliveryOutcome::SecureClipboardAttempted
        )));
        assert!(delivery_is_silent(&Err(
            TextDeliveryError::SecureDeliveryFailed("restricted".to_owned())
        )));
        assert!(!delivery_requires_copy_recovery(&Ok(
            TextDeliveryOutcome::AccessibilityVerified
        )));
        assert!(delivery_requires_copy_recovery(&Err(
            TextDeliveryError::NoFocusedControl
        )));
    }
}
