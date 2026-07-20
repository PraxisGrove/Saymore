use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::{ComponentHandle, Timer};
use std::time::Duration;
use windows::{
    Win32::{
        Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, WPARAM},
        Graphics::Dwm::{
            DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DwmSetWindowAttribute,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{ICON_BIG, ICON_SMALL, LoadIconW, SendMessageW, WM_SETICON},
    },
    core::PCWSTR,
};

use crate::ui::{AppColors, AppWindow, OnboardingWindow};

const TASKBAR_ICON_RESOURCE_ID: usize = 2;
const ONBOARDING_CAPTION_COLOR: COLORREF = COLORREF(0x00f4_f7f7);
const ONBOARDING_CAPTION_TEXT_COLOR: COLORREF = COLORREF(0x001c_1f1f);

pub(crate) fn integrate(ui: &AppWindow) {
    ui.window().set_size(slint::LogicalSize::new(920.0, 700.0));
    refresh(ui);
}

pub(crate) fn refresh(ui: &AppWindow) {
    let initial_ui = ui.as_weak();
    Timer::single_shot(Duration::from_millis(100), move || {
        let Some(ui) = initial_ui.upgrade() else {
            return;
        };
        if apply(&ui).is_ok() {
            return;
        }

        let retry_ui = ui.as_weak();
        Timer::single_shot(Duration::from_millis(400), move || {
            if let Some(ui) = retry_ui.upgrade()
                && let Err(error) = apply(&ui)
            {
                tracing::warn!(event = "main_window.windows_integration_failed", reason = %error);
            }
        });
    });
}

pub(crate) fn integrate_onboarding(ui: &OnboardingWindow) {
    let initial_ui = ui.as_weak();
    Timer::single_shot(Duration::from_millis(100), move || {
        let Some(ui) = initial_ui.upgrade() else {
            return;
        };
        if let Err(error) = apply_onboarding(ui.window()) {
            tracing::warn!(event = "onboarding.windows_integration_failed", reason = %error);
        }
    });
}

fn apply(ui: &AppWindow) -> Result<(), String> {
    let colors = ui.global::<AppColors>();
    apply_window(
        ui.window(),
        colorref(colors.get_canvas()),
        colorref(colors.get_ink()),
    )
}

fn apply_onboarding(window: &slint::Window) -> Result<(), String> {
    apply_window(
        window,
        ONBOARDING_CAPTION_COLOR,
        ONBOARDING_CAPTION_TEXT_COLOR,
    )
}

fn apply_window(
    window: &slint::Window,
    caption_color: COLORREF,
    caption_text_color: COLORREF,
) -> Result<(), String> {
    let window_handle = window.window_handle();
    let handle = window_handle
        .window_handle()
        .map_err(|error| error.to_string())?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return Err("the main window does not have a Win32 window handle".to_owned());
    };
    let hwnd = HWND(handle.hwnd.get() as *mut _);

    set_taskbar_icon(hwnd)?;
    set_dwm_color(hwnd, DWMWA_CAPTION_COLOR, caption_color)?;
    set_dwm_color(hwnd, DWMWA_TEXT_COLOR, caption_text_color)?;
    set_dwm_color(hwnd, DWMWA_BORDER_COLOR, caption_color)
}

fn colorref(color: slint::Color) -> COLORREF {
    let channels = color.to_argb_u8();
    COLORREF(
        u32::from(channels.red)
            | (u32::from(channels.green) << 8)
            | (u32::from(channels.blue) << 16),
    )
}

fn set_dwm_color(
    hwnd: HWND,
    attribute: windows::Win32::Graphics::Dwm::DWMWINDOWATTRIBUTE,
    color: COLORREF,
) -> Result<(), String> {
    // DWM copies the color during this call; the pointer does not escape.
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            attribute,
            (&raw const color).cast(),
            size_of::<COLORREF>() as u32,
        )
    }
    .map_err(|error| error.to_string())
}

fn set_taskbar_icon(hwnd: HWND) -> Result<(), String> {
    let module = unsafe { GetModuleHandleW(None) }.map_err(|error| error.to_string())?;
    let resource = PCWSTR(TASKBAR_ICON_RESOURCE_ID as *const u16);
    let icon = unsafe { LoadIconW(Some(HINSTANCE(module.0)), resource) }
        .map_err(|error| error.to_string())?;
    let icon_parameter = LPARAM(icon.0 as isize);

    unsafe {
        SendMessageW(
            hwnd,
            WM_SETICON,
            Some(WPARAM(ICON_BIG as usize)),
            Some(icon_parameter),
        );
        SendMessageW(
            hwnd,
            WM_SETICON,
            Some(WPARAM(ICON_SMALL as usize)),
            Some(icon_parameter),
        );
    }
    Ok(())
}
