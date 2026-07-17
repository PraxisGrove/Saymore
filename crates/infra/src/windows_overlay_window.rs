use std::ffi::c_void;

use thiserror::Error;
use windows::Win32::{
    Foundation::{GetLastError, HWND, POINT, RECT, SetLastError, WIN32_ERROR},
    Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint},
    UI::{
        HiDpi::{
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor, MDT_EFFECTIVE_DPI,
            SetThreadDpiAwarenessContext,
        },
        WindowsAndMessaging::{
            GWL_EXSTYLE, GetCursorPos, GetWindowLongPtrW, GetWindowRect, HWND_TOPMOST,
            SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOSIZE, SetWindowLongPtrW, SetWindowPos,
            WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
        },
    },
};

const BOTTOM_MARGIN: i32 = 12;
const DEFAULT_DPI: u32 = 96;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WindowsOverlayWindowError {
    #[error("the overlay does not have a valid Win32 window handle")]
    InvalidHandle,
    #[error("Windows could not configure the overlay: {0}")]
    Configure(String),
}

/// Configures an existing Slint/Winit overlay as a topmost nonactivating tool window.
pub fn configure_windows_overlay_window(hwnd: isize) -> Result<(), WindowsOverlayWindowError> {
    if hwnd == 0 {
        return Err(WindowsOverlayWindowError::InvalidHandle);
    }
    let hwnd = HWND(hwnd as *mut c_void);
    let previous_dpi_context =
        unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    if previous_dpi_context.0.is_null() {
        return Err(WindowsOverlayWindowError::Configure(
            "SetThreadDpiAwarenessContext failed".to_owned(),
        ));
    }
    let result = configure_per_monitor_aware(hwnd);
    unsafe { SetThreadDpiAwarenessContext(previous_dpi_context) };
    result
}

fn configure_per_monitor_aware(hwnd: HWND) -> Result<(), WindowsOverlayWindowError> {
    let current = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) };
    let overlay_bits = (WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST).0 as isize;
    unsafe { SetLastError(WIN32_ERROR(0)) };
    let previous = unsafe { SetWindowLongPtrW(hwnd, GWL_EXSTYLE, current | overlay_bits) };
    let style_error = unsafe { GetLastError() };
    if previous == 0 && style_error.0 != 0 {
        return Err(WindowsOverlayWindowError::Configure(format!(
            "SetWindowLongPtrW failed with Windows error {}",
            style_error.0
        )));
    }
    let origin = overlay_origin(hwnd)?;
    unsafe {
        SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            origin.0,
            origin.1,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOSIZE | SWP_FRAMECHANGED,
        )
    }
    .map_err(|error| WindowsOverlayWindowError::Configure(error.to_string()))
}

fn overlay_origin(hwnd: HWND) -> Result<(i32, i32), WindowsOverlayWindowError> {
    let mut cursor = POINT::default();
    unsafe { GetCursorPos(&raw mut cursor) }
        .map_err(|error| WindowsOverlayWindowError::Configure(error.to_string()))?;
    let monitor = unsafe { MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST) };
    let mut monitor_info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        rcMonitor: RECT::default(),
        rcWork: RECT::default(),
        dwFlags: 0,
    };
    if !unsafe { GetMonitorInfoW(monitor, &raw mut monitor_info) }.as_bool() {
        return Err(WindowsOverlayWindowError::Configure(
            "GetMonitorInfoW failed".to_owned(),
        ));
    }
    let mut dpi_x = DEFAULT_DPI;
    let mut dpi_y = DEFAULT_DPI;
    unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &raw mut dpi_x, &raw mut dpi_y) }
        .map_err(|error| WindowsOverlayWindowError::Configure(error.to_string()))?;
    let margin =
        BOTTOM_MARGIN * i32::try_from(dpi_y).unwrap_or(DEFAULT_DPI as i32) / DEFAULT_DPI as i32;
    let mut window_rect = RECT::default();
    unsafe { GetWindowRect(hwnd, &raw mut window_rect) }
        .map_err(|error| WindowsOverlayWindowError::Configure(error.to_string()))?;
    Ok(bottom_center_origin(
        (
            monitor_info.rcWork.left,
            monitor_info.rcWork.top,
            monitor_info.rcWork.right,
            monitor_info.rcWork.bottom,
        ),
        (
            window_rect.right - window_rect.left,
            window_rect.bottom - window_rect.top,
        ),
        margin,
    ))
}

fn bottom_center_origin(
    work_area: (i32, i32, i32, i32),
    window_size: (i32, i32),
    bottom_margin: i32,
) -> (i32, i32) {
    let work_width = work_area.2 - work_area.0;
    (
        work_area.0 + (work_width - window_size.0) / 2,
        work_area.3 - window_size.1 - bottom_margin,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_missing_window_handle() {
        assert_eq!(
            Err(WindowsOverlayWindowError::InvalidHandle),
            configure_windows_overlay_window(0)
        );
    }

    #[test]
    fn positions_overlay_at_bottom_center_of_work_area() {
        assert_eq!(
            (900, 993),
            bottom_center_origin((0, 0, 1920, 1040), (120, 35), 12)
        );
    }

    #[test]
    fn positions_overlay_on_work_areas_with_negative_origins() {
        assert_eq!(
            (-1660, 1025),
            bottom_center_origin((-2560, 0, -640, 1080), (120, 43), 12)
        );
    }
}
