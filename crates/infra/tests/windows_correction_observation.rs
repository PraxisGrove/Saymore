#![cfg(target_os = "windows")]

use std::{sync::mpsc, thread, time::Duration};

use template_app::{CorrectionObservingTextDeliverer, correction_from_edit};
use template_infra::WindowsTextDeliverer;

#[test]
#[ignore = "requires a focused editable Notepad document and a manual correction"]
fn notepad_reports_a_user_correction_after_real_delivery() -> Result<(), Box<dyn std::error::Error>>
{
    let deliverer = WindowsTextDeliverer::new()?;
    eprintln!("focus a writable Notepad document now");
    thread::sleep(Duration::from_secs(5));

    let (sender, receiver) = mpsc::sync_channel(1);
    let outcome = deliverer.deliver_and_observe(
        "We use CMO for development",
        Box::new(move |edit| {
            let _ = sender.send(edit);
        }),
    )?;
    eprintln!("delivery outcome: {outcome:?}; replace CMO with Saymore in Notepad now");

    let edit = receiver.recv_timeout(Duration::from_secs(25))?;
    let correction = correction_from_edit(&edit.original, &edit.edited)
        .ok_or("the observed Notepad change was not an eligible local correction")?;
    if correction.canonical == "Saymore" {
        Ok(())
    } else {
        Err(format!("expected Saymore, observed {}", correction.canonical).into())
    }
}
