#[cfg(target_os = "windows")]
use std::{env, error::Error, io, thread, time::Duration};

#[cfg(target_os = "windows")]
use template_app::TextDeliverer;
#[cfg(target_os = "windows")]
use template_infra::WindowsTextDeliverer;
#[cfg(target_os = "windows")]
use windows::Win32::{
    System::{
        Com::{CLSCTX_INPROC_SERVER, CoCreateInstance},
        Ole::{OleInitialize, OleUninitialize},
    },
    UI::Accessibility::{CUIAutomation, IUIAutomation},
};

#[cfg(target_os = "windows")]
fn main() -> Result<(), Box<dyn Error>> {
    let mut arguments = env::args().skip(1);
    let wait_ms = arguments
        .next()
        .ok_or_else(usage)?
        .parse::<u64>()
        .map_err(|_| usage())?;
    let text = arguments.next().ok_or_else(usage)?;
    if arguments.next().is_some() || text.is_empty() {
        return Err(usage().into());
    }

    println!("Delivery text: {text:?}");
    println!("Focus the target text control; delivery starts in {wait_ms} ms.");
    thread::sleep(Duration::from_millis(wait_ms));
    print_focused_control()?;
    let deliverer = WindowsTextDeliverer::new()?;
    let outcome = deliverer.deliver(&text)?;
    println!("PASS: {outcome:?}");
    Ok(())
}

#[cfg(target_os = "windows")]
fn print_focused_control() -> Result<(), Box<dyn Error>> {
    unsafe { OleInitialize(None) }?;
    let result = (|| {
        let automation: IUIAutomation =
            unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }?;
        let element = unsafe { automation.GetFocusedElement() }?;
        let class_name = unsafe { element.CurrentClassName() }?;
        let window = unsafe { element.CurrentNativeWindowHandle() }?;
        println!("Focused class: {class_name}; native handle: {:?}", window.0);
        Ok::<(), windows::core::Error>(())
    })();
    unsafe { OleUninitialize() };
    result.map_err(Into::into)
}

#[cfg(target_os = "windows")]
fn usage() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "usage: windows_text_delivery_probe WAIT_MS TEXT",
    )
}

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("windows_text_delivery_probe is available only on Windows");
}
