use std::{mem, ptr, thread, time::Duration};

use template_app::TextDeliveryError;
use windows::{
    Win32::{
        Foundation::{GlobalFree, HANDLE},
        System::{
            Com::IDataObject,
            DataExchange::{
                CloseClipboard, EmptyClipboard, GetClipboardSequenceNumber, OpenClipboard,
                SetClipboardData,
            },
            Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock},
            Ole::{CF_UNICODETEXT, OleGetClipboard, OleSetClipboard},
        },
    },
    core::Error as WindowsError,
};

use super::ClipboardSetupFailure;

const RETRY_DELAY: Duration = Duration::from_millis(10);
const RETRIES: usize = 10;

pub(super) struct ClipboardSnapshot(IDataObject);

#[derive(Debug, Clone, Copy)]
pub(super) struct TemporaryClipboard {
    sequence: u32,
}

pub(super) fn snapshot() -> Result<ClipboardSnapshot, TextDeliveryError> {
    retry_windows("snapshot clipboard", || unsafe { OleGetClipboard() }).map(ClipboardSnapshot)
}

pub(super) fn replace_with_text(
    text: &str,
) -> Result<TemporaryClipboard, ClipboardSetupFailure<TemporaryClipboard>> {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = wide.len().saturating_mul(mem::size_of::<u16>());
    let memory = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes) }
        .map_err(|error| setup_failure(system_error("allocate clipboard text", error), None))?;
    let destination = unsafe { GlobalLock(memory) }.cast::<u16>();
    if destination.is_null() {
        let _ = unsafe { GlobalFree(Some(memory)) };
        return Err(setup_failure(
            TextDeliveryError::System("could not lock clipboard text memory".to_owned()),
            None,
        ));
    }
    unsafe { ptr::copy_nonoverlapping(wide.as_ptr(), destination, wide.len()) };
    let _ = unsafe { GlobalUnlock(memory) };

    if let Err(error) = open_clipboard() {
        let _ = unsafe { GlobalFree(Some(memory)) };
        return Err(setup_failure(error, None));
    }
    if let Err(error) = unsafe { EmptyClipboard() } {
        close_after_failure();
        let _ = unsafe { GlobalFree(Some(memory)) };
        return Err(setup_failure(system_error("empty clipboard", error), None));
    }
    if let Err(error) = unsafe { SetClipboardData(CF_UNICODETEXT.0.into(), Some(HANDLE(memory.0))) }
    {
        let sequence = unsafe { GetClipboardSequenceNumber() };
        close_after_failure();
        let _ = unsafe { GlobalFree(Some(memory)) };
        return Err(setup_failure(
            system_error("set clipboard text", error),
            Some(TemporaryClipboard { sequence }),
        ));
    }
    let sequence = unsafe { GetClipboardSequenceNumber() };
    if let Err(error) = close_clipboard() {
        return Err(setup_failure(error, Some(TemporaryClipboard { sequence })));
    }
    if sequence == 0 {
        return Err(setup_failure(
            TextDeliveryError::System("clipboard sequence number is unavailable".to_owned()),
            Some(TemporaryClipboard { sequence }),
        ));
    }
    Ok(TemporaryClipboard { sequence })
}

fn setup_failure(
    error: TextDeliveryError,
    temporary: Option<TemporaryClipboard>,
) -> ClipboardSetupFailure<TemporaryClipboard> {
    ClipboardSetupFailure { error, temporary }
}

pub(super) fn restore_if_unchanged(
    snapshot: ClipboardSnapshot,
    temporary: TemporaryClipboard,
) -> Result<(), TextDeliveryError> {
    let mut last_error = None;
    for _ in 0..RETRIES {
        let current = unsafe { GetClipboardSequenceNumber() };
        if temporary.sequence != 0 && current != 0 && !should_restore(temporary.sequence, current) {
            return Ok(());
        }
        match unsafe { OleSetClipboard(&snapshot.0) } {
            Ok(()) => return Ok(()),
            Err(error) => {
                last_error = Some(error);
                thread::sleep(RETRY_DELAY);
            }
        }
    }
    Err(last_error.map_or_else(
        || TextDeliveryError::System("restore clipboard failed".to_owned()),
        |error| system_error("restore clipboard", error),
    ))
}

fn should_restore(temporary_sequence: u32, current_sequence: u32) -> bool {
    temporary_sequence == current_sequence
}

fn open_clipboard() -> Result<(), TextDeliveryError> {
    retry_windows("open clipboard", || unsafe { OpenClipboard(None) })
}

fn close_clipboard() -> Result<(), TextDeliveryError> {
    retry_windows("close clipboard", || unsafe { CloseClipboard() })
}

fn retry_windows<T>(
    operation: &str,
    mut action: impl FnMut() -> windows::core::Result<T>,
) -> Result<T, TextDeliveryError> {
    let mut last_error = None;
    for _ in 0..RETRIES {
        match action() {
            Ok(value) => return Ok(value),
            Err(error) => {
                last_error = Some(error);
                thread::sleep(RETRY_DELAY);
            }
        }
    }
    Err(last_error.map_or_else(
        || TextDeliveryError::System(format!("{operation} failed")),
        |error| system_error(operation, error),
    ))
}

fn close_after_failure() {
    let _ = close_clipboard();
}

fn system_error(operation: &str, error: WindowsError) -> TextDeliveryError {
    TextDeliveryError::System(format!("{operation} failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_only_when_temporary_clipboard_is_still_current() {
        assert!(should_restore(42, 42));
        assert!(!should_restore(42, 43));
    }
}
