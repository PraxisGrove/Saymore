use slint::{ComponentHandle, SharedString};

use crate::ui::{
    AccessibilityPermissionOverlay, AppWindow, AsrConfigurationOverlay, DictionaryAddedOverlay,
    MicrophoneIntroOverlay, MicrophonePermissionOverlay, OnboardingWindow, RecordingLimitOverlay,
    RecordingOverlay, ResultOverlay, Typography,
};

pub(crate) trait ApplyTypography {
    fn apply_typography(&self);
}

macro_rules! impl_apply_typography {
    ($($component:ty),+ $(,)?) => {
        $(
            impl ApplyTypography for $component {
                fn apply_typography(&self) {
                    let typography = self.global::<Typography>();
                    typography.set_ui_font_family(SharedString::from(ui_font_family()));
                    typography.set_mono_font_family(SharedString::from(mono_font_family()));
                }
            }
        )+
    };
}

impl_apply_typography!(
    AccessibilityPermissionOverlay,
    AppWindow,
    AsrConfigurationOverlay,
    DictionaryAddedOverlay,
    MicrophoneIntroOverlay,
    MicrophonePermissionOverlay,
    OnboardingWindow,
    RecordingLimitOverlay,
    RecordingOverlay,
    ResultOverlay,
);

#[cfg(target_os = "windows")]
const fn ui_font_family() -> &'static str {
    "Segoe UI"
}

#[cfg(target_os = "macos")]
const fn ui_font_family() -> &'static str {
    "PingFang SC"
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
const fn ui_font_family() -> &'static str {
    "sans-serif"
}

#[cfg(target_os = "windows")]
const fn mono_font_family() -> &'static str {
    "Consolas"
}

#[cfg(target_os = "macos")]
const fn mono_font_family() -> &'static str {
    "Menlo"
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
const fn mono_font_family() -> &'static str {
    "monospace"
}
