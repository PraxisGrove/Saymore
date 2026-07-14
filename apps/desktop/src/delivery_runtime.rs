use std::time::Duration;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::{ComponentHandle, SharedString, Timer};
use template_app::{
    FeedbackSound, FeedbackSoundPlayer, PcmRecording, ProcessedText, RefinementStatus,
    TextDeliverer, TextDeliveryOutcome,
};
use template_infra::{
    MacOsFeedbackSoundPlayer, MacOsTextDeliverer, configure_overlay_window, copy_text_to_clipboard,
};

use crate::{
    ui::{AppWindow, RecordingOverlay, ResultOverlay},
    ui_status::{apply_transcription_completed, delivery_requires_copy_recovery},
};

const FOCUS_SETTLE_DELAY: Duration = Duration::from_millis(80);
const FALLBACK_NOTICE_DELAY: Duration = Duration::from_millis(900);

pub fn wire_result_actions(overlay: &ResultOverlay) {
    let copy_overlay = overlay.as_weak();
    overlay.on_copy_result(move || {
        let Some(overlay) = copy_overlay.upgrade() else {
            return;
        };
        if copy_text_to_clipboard(overlay.get_transcript().as_str()).is_ok() {
            let _ = overlay.hide();
        }
    });

    let close_overlay = overlay.as_weak();
    overlay.on_close_result(move || {
        if let Some(overlay) = close_overlay.upgrade() {
            let _ = overlay.hide();
        }
    });
}

pub fn schedule_delivery(
    ui: slint::Weak<AppWindow>,
    status_overlay: slint::Weak<RecordingOverlay>,
    result_overlay: slint::Weak<ResultOverlay>,
    recording: PcmRecording,
    processed: ProcessedText,
) {
    let fell_back = matches!(processed.refinement, RefinementStatus::FellBack(_));
    if let Some(overlay) = status_overlay.upgrade() {
        if fell_back {
            overlay.set_mode(1);
            overlay.set_processing_label(SharedString::from("润色未完成，已使用转写结果"));
        } else {
            let _ = overlay.hide();
        }
    }

    let delivery_delay = if fell_back {
        FALLBACK_NOTICE_DELAY
    } else {
        FOCUS_SETTLE_DELAY
    };
    Timer::single_shot(delivery_delay, move || {
        if let Some(overlay) = status_overlay.upgrade() {
            let _ = overlay.hide();
        }
        let delivery = MacOsTextDeliverer.deliver(&processed.text);
        let requires_recovery = delivery_requires_copy_recovery(&delivery);
        let verified = matches!(
            delivery,
            Ok(TextDeliveryOutcome::AccessibilityVerified | TextDeliveryOutcome::ClipboardVerified)
        );

        let sound = if verified {
            FeedbackSound::Finish
        } else {
            FeedbackSound::Failure
        };
        let _ = MacOsFeedbackSoundPlayer.play(sound);

        if let Some(ui) = ui.upgrade() {
            apply_transcription_completed(&ui, &recording, &processed, delivery);
        }
        if requires_recovery && let Some(overlay) = result_overlay.upgrade() {
            show_result_overlay(&overlay, &processed.text);
        }
    });
}

fn show_result_overlay(overlay: &ResultOverlay, transcript: &str) {
    overlay.set_transcript(SharedString::from(transcript));
    if overlay.show().is_ok() {
        position_result_overlay(overlay);
    }
}

fn position_result_overlay(overlay: &ResultOverlay) {
    let handle = overlay.window().window_handle();
    let result = handle
        .window_handle()
        .map_err(|error| error.to_string())
        .and_then(|handle| match handle.as_raw() {
            RawWindowHandle::AppKit(handle) => unsafe {
                configure_overlay_window(handle.ns_view).map_err(|error| error.to_string())
            },
            _ => Err("the result overlay does not have an AppKit window handle".to_owned()),
        });
    if let Err(error) = result {
        eprintln!("failed to position result overlay: {error}");
    }
}
