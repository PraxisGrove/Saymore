#![cfg(target_os = "windows")]

use std::{sync::mpsc, thread, time::Duration};

use template_app::{
    CorrectionObservingTextDeliverer, FinalTextRevisionState, correction_from_edit,
};
use template_infra::WindowsTextDeliverer;

#[test]
#[ignore = "requires a focused editable Notepad document and a manual correction"]
fn notepad_reports_a_user_correction_after_real_delivery() -> Result<(), Box<dyn std::error::Error>>
{
    let deliverer = WindowsTextDeliverer::new()?;
    eprintln!("focus a writable Notepad document now");
    thread::sleep(Duration::from_secs(5));

    let (sender, receiver) = mpsc::sync_channel(1);
    let original = "We use CMO for development";
    let outcome = deliverer.deliver_and_observe(
        original,
        Box::new(move |edit| {
            let _ = sender.send(edit);
        }),
    )?;
    eprintln!("delivery outcome: {outcome:?}; replace CMO with Saymore in Notepad now");

    let mut state = FinalTextRevisionState::new(original);
    let revision = loop {
        let event = receiver.recv_timeout(Duration::from_secs(130))?;
        if let Some(revision) = state.handle(event) {
            break revision;
        }
    };
    let correction = correction_from_edit(&revision.original, &revision.final_text)
        .ok_or("the observed Notepad change was not an eligible local correction")?;
    if correction.canonical == "Saymore" {
        Ok(())
    } else {
        Err(format!("expected Saymore, observed {}", correction.canonical).into())
    }
}
