#[cfg(target_os = "windows")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::{
        sync::{Arc, mpsc},
        thread,
        time::Duration,
    };

    use template_infra::{
        DictationShortcutAction, WindowsShortcut, WindowsShortcutController, WindowsShortcutMonitor,
    };
    use windows::Win32::UI::{
        Input::KeyboardAndMouse::{INPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, SendInput},
        WindowsAndMessaging::GetForegroundWindow,
    };

    let wait_ms = std::env::args()
        .nth(1)
        .map(|value| value.parse::<u64>())
        .transpose()?
        .unwrap_or(1_500);
    let controller = WindowsShortcutController::new(vec![WindowsShortcut::default()]);
    let (actions, received_actions) = mpsc::sync_channel(1);
    let mut monitor = WindowsShortcutMonitor::start(
        Arc::new(|| false),
        Arc::new(|| true),
        controller,
        move |action| {
            let _ = actions.try_send(action);
        },
    )?;

    println!("Right Alt hook ready; injecting in {wait_ms} ms.");
    thread::sleep(Duration::from_millis(wait_ms));
    let foreground_before = unsafe { GetForegroundWindow() };
    let focus_before = focused_window(foreground_before)?;
    let inputs = [
        keyboard_input(KEYEVENTF_EXTENDEDKEY),
        keyboard_input(KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP),
    ];
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent != inputs.len() as u32 {
        monitor.shutdown();
        return Err(format!("SendInput accepted {sent} of {} events", inputs.len()).into());
    }
    let action = received_actions.recv_timeout(Duration::from_secs(2))?;
    thread::sleep(Duration::from_millis(150));
    let foreground_after = unsafe { GetForegroundWindow() };
    let focus_after = focused_window(foreground_after)?;
    monitor.shutdown();

    println!("Action: {action:?}");
    println!("Foreground before: {:?}", foreground_before.0);
    println!("Foreground after: {:?}", foreground_after.0);
    println!("Focused control before: {:?}", focus_before.0);
    println!("Focused control after: {:?}", focus_after.0);
    if action != DictationShortcutAction::Toggle {
        return Err("Right Alt did not produce exactly one toggle action".into());
    }
    if foreground_before != foreground_after {
        return Err("Right Alt changed the foreground window".into());
    }
    if focus_before != focus_after {
        return Err("Right Alt moved focus inside the foreground window".into());
    }
    println!("PASS: Right Alt was reserved for Saymore without changing foreground window.");
    Ok(())
}

#[cfg(target_os = "windows")]
fn focused_window(
    foreground: windows::Win32::Foundation::HWND,
) -> Result<windows::Win32::Foundation::HWND, Box<dyn std::error::Error>> {
    use windows::Win32::UI::WindowsAndMessaging::{
        GUITHREADINFO, GetGUIThreadInfo, GetWindowThreadProcessId,
    };

    let thread_id = unsafe { GetWindowThreadProcessId(foreground, None) };
    let mut info = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    unsafe { GetGUIThreadInfo(thread_id, &mut info) }?;
    Ok(info.hwndFocus)
}

#[cfg(target_os = "windows")]
fn keyboard_input(
    flags: windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS,
) -> windows::Win32::UI::Input::KeyboardAndMouse::INPUT {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, VK_RMENU,
    };

    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VK_RMENU,
                dwFlags: flags,
                ..Default::default()
            },
        },
    }
}

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("windows_right_alt_probe is only available on Windows");
}
