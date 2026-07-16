#![cfg(target_os = "macos")]

use std::{sync::mpsc, time::Duration};

use template_app::{
    AccessibilityAuthorization, CorrectionObservingTextDeliverer, TextDeliverer,
    correction_from_edit,
};
use template_infra::MacOsTextDeliverer;

#[test]
#[ignore = "requires a focused editable TextEdit document and a manual correction"]
fn textedit_reports_a_user_correction_after_real_delivery() -> Result<(), Box<dyn std::error::Error>>
{
    let deliverer = MacOsTextDeliverer;
    if deliverer.authorization() != AccessibilityAuthorization::Granted {
        return Err("Accessibility permission is not granted".into());
    }
    let (sender, receiver) = mpsc::sync_channel(1);
    let outcome = deliverer.deliver_and_observe(
        "我们使用 CMO 开发",
        Box::new(move |edit| {
            let _ = sender.send(edit);
        }),
    )?;
    eprintln!("delivery outcome: {outcome:?}; replace CMO with Saymore in TextEdit now");

    let edit = receiver.recv_timeout(Duration::from_secs(25))?;
    let correction = correction_from_edit(&edit.original, &edit.edited)
        .ok_or("the observed TextEdit change was not an eligible local correction")?;
    if correction.canonical == "Saymore" {
        Ok(())
    } else {
        Err(format!("expected Saymore, observed {}", correction.canonical).into())
    }
}
