use slint::SharedString;
use template_app::{
    AccessibilityAuthorization, MicrophoneAuthorization, PcmRecording, ProcessedText,
    RecordingError, RefinementStatus, SpeechRecognitionError, TextDeliveryError,
    TextDeliveryOutcome,
};

use crate::{asr_runtime, ui::AppWindow};

pub fn apply_recording_started(ui: &AppWindow) {
    ui.set_recording_active(true);
    ui.set_recording_failed(false);
    ui.set_recording_complete(false);
    ui.set_recording_attempted(false);
    ui.set_recording_level(0.0);
    ui.set_recording_status(SharedString::from("正在录音"));
    ui.set_recording_detail(SharedString::from("0.0 秒 · 0 个输入采样"));
}

pub fn apply_transcription_completed(
    ui: &AppWindow,
    recording: &PcmRecording,
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
            ui.set_recording_status(SharedString::from(delivery_status(
                &processed.refinement,
                outcome,
            )));
            ui.set_recording_detail(SharedString::from(completion_detail(
                recording, processed, outcome,
            )));
        }
        Err(error) => {
            ui.set_recording_status(SharedString::from("投递失败"));
            ui.set_recording_detail(SharedString::from(text_delivery_error_message(&error)));
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

fn completion_status(refinement: &RefinementStatus) -> &'static str {
    match refinement {
        RefinementStatus::Disabled => "转写完成",
        RefinementStatus::Skipped(_) => "转写完成",
        RefinementStatus::Completed => "润色完成",
        RefinementStatus::FellBack(_) => "润色未完成",
    }
}

fn delivery_status(refinement: &RefinementStatus, outcome: TextDeliveryOutcome) -> &'static str {
    match outcome {
        TextDeliveryOutcome::AccessibilityVerified | TextDeliveryOutcome::ClipboardVerified => {
            completion_status(refinement)
        }
        TextDeliveryOutcome::ClipboardAttempted => "已尝试投递",
        TextDeliveryOutcome::SecureClipboardAttempted => "已尝试安全输入",
    }
}

fn completion_detail(
    recording: &PcmRecording,
    processed: &ProcessedText,
    outcome: TextDeliveryOutcome,
) -> String {
    if let RefinementStatus::FellBack(reason) = &processed.refinement {
        return fallback_detail(reason).to_owned();
    }
    format!(
        "{} · {} 字 · {}",
        format_duration(recording.duration_ms),
        processed.text.chars().count(),
        delivery_outcome_label(outcome)
    )
}

pub fn fallback_detail(reason: &template_app::RefinementFallbackReason) -> &'static str {
    use template_app::RefinementFallbackReason;

    match reason {
        RefinementFallbackReason::Timeout
        | RefinementFallbackReason::Transport
        | RefinementFallbackReason::Quota
        | RefinementFallbackReason::TemporarilyUnavailable => "润色服务暂时不可用，已使用转写结果",
        RefinementFallbackReason::OutputRejected => "润色结果异常，已使用转写结果",
        RefinementFallbackReason::NotConfigured
        | RefinementFallbackReason::Authentication
        | RefinementFallbackReason::InvalidConfiguration
        | RefinementFallbackReason::ModelUnavailable
        | RefinementFallbackReason::Protocol => "润色未完成，已使用转写结果",
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
    ui.set_recording_status(SharedString::from("识别失败"));
    ui.set_recording_detail(SharedString::from(asr_runtime::error_message(error)));
}

pub fn apply_recording_error(ui: &AppWindow, error: &RecordingError) {
    ui.set_recording_active(false);
    ui.set_recording_failed(true);
    ui.set_recording_complete(false);
    ui.set_recording_attempted(false);
    ui.set_recording_level(0.0);
    ui.set_recording_status(SharedString::from("录音失败"));
    ui.set_recording_detail(SharedString::from(recording_error_message(error)));
}

pub fn format_duration(milliseconds: u64) -> String {
    format!(
        "{}.{:01} 秒",
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
    ui.set_authorization_status(SharedString::from(if granted {
        "已授权"
    } else {
        "未授权"
    }));
}

pub fn update_microphone_authorization(ui: &AppWindow, authorization: MicrophoneAuthorization) {
    let granted = authorization == MicrophoneAuthorization::Granted;
    ui.set_microphone_authorized(granted);
    ui.set_microphone_status(SharedString::from(match authorization {
        MicrophoneAuthorization::NotDetermined => "未请求",
        MicrophoneAuthorization::Granted => "已授权",
        MicrophoneAuthorization::Denied => "已拒绝",
        MicrophoneAuthorization::Restricted => "受系统限制",
    }));
}

fn delivery_outcome_label(outcome: TextDeliveryOutcome) -> &'static str {
    match outcome {
        TextDeliveryOutcome::AccessibilityVerified => "已直接写入",
        TextDeliveryOutcome::ClipboardVerified => "已粘贴",
        TextDeliveryOutcome::ClipboardAttempted => "已发出粘贴",
        TextDeliveryOutcome::SecureClipboardAttempted => "已尝试安全输入",
    }
}

fn text_delivery_error_message(error: &TextDeliveryError) -> &'static str {
    match error {
        TextDeliveryError::PermissionDenied => "需要辅助功能权限",
        TextDeliveryError::NoFocusedControl => "没有找到可输入的位置",
        TextDeliveryError::UnsupportedControl => "当前控件不支持文字投递",
        TextDeliveryError::AccessibilityUnverified => "无法确认文字是否写入",
        TextDeliveryError::SecureDeliveryFailed(_) => "安全输入投递失败",
        TextDeliveryError::System(_) => "系统文字投递失败",
    }
}

fn recording_error_message(error: &RecordingError) -> &'static str {
    match error {
        RecordingError::PermissionDenied => "需要麦克风权限",
        RecordingError::NoInputDevice => "没有找到默认麦克风",
        RecordingError::AlreadyRecording => "录音已经开始",
        RecordingError::NotRecording => "录音尚未开始",
        RecordingError::UnsupportedSampleFormat(_) => "麦克风采样格式暂不支持",
        RecordingError::Capture(_) => "麦克风采集失败",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use template_app::RefinementFallbackReason;

    #[test]
    fn formats_recording_duration_to_tenths() {
        assert_eq!("0.0 秒", format_duration(0));
        assert_eq!("1.2 秒", format_duration(1_250));
        assert_eq!("61.0 秒", format_duration(61_099));
    }

    #[test]
    fn maps_recording_errors_to_actionable_messages() {
        assert_eq!(
            ["需要麦克风权限", "没有找到默认麦克风", "麦克风采集失败"],
            [
                recording_error_message(&RecordingError::PermissionDenied),
                recording_error_message(&RecordingError::NoInputDevice),
                recording_error_message(&RecordingError::Capture("device stopped".to_owned())),
            ]
        );
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
        assert_eq!(
            "已尝试安全输入",
            delivery_outcome_label(TextDeliveryOutcome::SecureClipboardAttempted)
        );
    }

    #[test]
    fn completion_copy_reflects_the_selected_processing_mode_and_fallback() {
        let recording = PcmRecording {
            samples: Vec::new(),
            sample_rate: 16_000,
            channels: 1,
            duration_ms: 1_250,
        };
        let disabled = ProcessedText {
            text: "转写文本".to_owned(),
            refinement: RefinementStatus::Disabled,
        };
        let completed = ProcessedText {
            text: "润色文本".to_owned(),
            refinement: RefinementStatus::Completed,
        };
        let skipped = ProcessedText {
            text: "短转写文本".to_owned(),
            refinement: RefinementStatus::Skipped(
                template_app::RefinementSkipReason::ShortTranscript,
            ),
        };
        let fallback = ProcessedText {
            text: "转写文本".to_owned(),
            refinement: RefinementStatus::FellBack(RefinementFallbackReason::Timeout),
        };

        assert_eq!("转写完成", completion_status(&disabled.refinement));
        assert_eq!("润色完成", completion_status(&completed.refinement));
        assert_eq!("转写完成", completion_status(&skipped.refinement));
        assert_eq!("润色未完成", completion_status(&fallback.refinement));
        assert_eq!(
            "润色服务暂时不可用，已使用转写结果",
            completion_detail(
                &recording,
                &fallback,
                TextDeliveryOutcome::AccessibilityVerified
            )
        );
    }

    #[test]
    fn unverified_delivery_uses_an_attempted_status() {
        assert_eq!(
            "已尝试投递",
            delivery_status(
                &RefinementStatus::Completed,
                TextDeliveryOutcome::ClipboardAttempted
            )
        );
        assert_eq!(
            "已尝试安全输入",
            delivery_status(
                &RefinementStatus::Completed,
                TextDeliveryOutcome::SecureClipboardAttempted
            )
        );
    }
}
