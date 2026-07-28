use slint::{ComponentHandle, SharedString};
use template_app::{ColorSchemePreference, LocalSettings, LocalSettingsChange, ThemeId};

use crate::{
    local_settings_runtime::LocalSettingsHandle,
    ui::{
        AppColors, AppWindow, ColorSchemePreference as UiColorSchemePreference, OnboardingWindow,
        ThemeId as UiThemeId, Translations,
    },
};

#[derive(Debug, Clone, Copy, PartialEq)]
struct UiAppearance {
    theme: UiThemeId,
    color_scheme: UiColorSchemePreference,
}

/// Receives the persisted appearance roles shared by application-owned windows.
/// Implementations must update only their own Slint appearance state.
trait AppearanceTarget {
    fn apply_appearance(&self, appearance: UiAppearance);
}

impl AppearanceTarget for AppWindow {
    fn apply_appearance(&self, appearance: UiAppearance) {
        self.set_theme_id(appearance.theme);
        self.set_color_scheme(appearance.color_scheme);
        self.set_appearance_status(SharedString::new());
        self.global::<AppColors>().set_theme_id(appearance.theme);
        self.global::<AppColors>()
            .set_color_scheme(appearance.color_scheme);
    }
}

impl AppearanceTarget for OnboardingWindow {
    fn apply_appearance(&self, appearance: UiAppearance) {
        self.global::<AppColors>().set_theme_id(appearance.theme);
        self.global::<AppColors>()
            .set_color_scheme(appearance.color_scheme);
    }
}

pub fn wire(
    ui: &AppWindow,
    onboarding: &OnboardingWindow,
    initial: &LocalSettings,
    settings: LocalSettingsHandle,
) {
    apply_to_windows(ui, onboarding, initial);
    wire_theme(ui, onboarding, settings.clone());
    wire_color_scheme(ui, onboarding, settings);
}

fn wire_theme(ui: &AppWindow, onboarding: &OnboardingWindow, settings: LocalSettingsHandle) {
    let app = ui.as_weak();
    let onboarding = onboarding.as_weak();
    ui.on_set_theme(move |theme| {
        let completion_app = app.clone();
        let completion_onboarding = onboarding.clone();
        let failure_ui = app.clone();
        let result = settings.submit(
            LocalSettingsChange::SetTheme(theme_from_ui(theme)),
            move |result| match result {
                Ok(committed) => {
                    apply_to_weak_windows(&completion_app, &completion_onboarding, &committed)
                }
                Err(error) => apply_error(&completion_app, "theme", error),
            },
        );
        if let Err(error) = result {
            apply_error(&failure_ui, "theme", error);
        }
    });
}

fn wire_color_scheme(ui: &AppWindow, onboarding: &OnboardingWindow, settings: LocalSettingsHandle) {
    let app = ui.as_weak();
    let onboarding = onboarding.as_weak();
    ui.on_set_color_scheme(move |scheme| {
        let completion_app = app.clone();
        let completion_onboarding = onboarding.clone();
        let failure_ui = app.clone();
        let result = settings.submit(
            LocalSettingsChange::SetColorScheme(color_scheme_from_ui(scheme)),
            move |result| match result {
                Ok(committed) => {
                    apply_to_weak_windows(&completion_app, &completion_onboarding, &committed)
                }
                Err(error) => apply_error(&completion_app, "color_scheme", error),
            },
        );
        if let Err(error) = result {
            apply_error(&failure_ui, "color_scheme", error);
        }
    });
}

fn apply_to_weak_windows(
    app: &slint::Weak<AppWindow>,
    onboarding: &slint::Weak<OnboardingWindow>,
    settings: &LocalSettings,
) {
    if let (Some(app), Some(onboarding)) = (app.upgrade(), onboarding.upgrade()) {
        apply_to_windows(&app, &onboarding, settings);
    }
}

fn apply_to_windows(app: &AppWindow, onboarding: &OnboardingWindow, settings: &LocalSettings) {
    let appearance = ui_appearance(settings);
    apply(app, appearance);
    apply(onboarding, appearance);
    crate::main_window::schedule_titlebar_integration(app);
    #[cfg(target_os = "windows")]
    crate::windows_window::refresh_onboarding(onboarding);
}

fn apply(target: &impl AppearanceTarget, appearance: UiAppearance) {
    target.apply_appearance(appearance);
}

fn ui_appearance(settings: &LocalSettings) -> UiAppearance {
    UiAppearance {
        theme: theme_to_ui(settings.theme),
        color_scheme: color_scheme_to_ui(settings.color_scheme),
    }
}

fn apply_error(
    ui: &slint::Weak<AppWindow>,
    operation: &'static str,
    error: impl std::fmt::Display,
) {
    tracing::warn!(event = "appearance.save_failed", operation, reason = %error);
    if let Some(ui) = ui.upgrade() {
        ui.set_appearance_status(ui.global::<Translations>().get_settings_save_failed());
    }
}

fn theme_to_ui(theme: ThemeId) -> UiThemeId {
    match theme {
        ThemeId::Saymore => UiThemeId::Saymore,
        ThemeId::WarmClay => UiThemeId::WarmClay,
        ThemeId::LimePulse => UiThemeId::LimePulse,
        ThemeId::BerryGraphite => UiThemeId::BerryGraphite,
        ThemeId::IrisMist => UiThemeId::IrisMist,
        ThemeId::SunlitGold => UiThemeId::SunlitGold,
    }
}

fn theme_from_ui(theme: UiThemeId) -> ThemeId {
    match theme {
        UiThemeId::Saymore => ThemeId::Saymore,
        UiThemeId::WarmClay => ThemeId::WarmClay,
        UiThemeId::LimePulse => ThemeId::LimePulse,
        UiThemeId::BerryGraphite => ThemeId::BerryGraphite,
        UiThemeId::IrisMist => ThemeId::IrisMist,
        UiThemeId::SunlitGold => ThemeId::SunlitGold,
    }
}

fn color_scheme_to_ui(scheme: ColorSchemePreference) -> UiColorSchemePreference {
    match scheme {
        ColorSchemePreference::System => UiColorSchemePreference::System,
        ColorSchemePreference::Light => UiColorSchemePreference::Light,
        ColorSchemePreference::Dark => UiColorSchemePreference::Dark,
    }
}

fn color_scheme_from_ui(scheme: UiColorSchemePreference) -> ColorSchemePreference {
    match scheme {
        UiColorSchemePreference::System => ColorSchemePreference::System,
        UiColorSchemePreference::Light => ColorSchemePreference::Light,
        UiColorSchemePreference::Dark => ColorSchemePreference::Dark,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[derive(Default)]
    struct RecordedTarget {
        appearance: Cell<Option<UiAppearance>>,
    }

    impl AppearanceTarget for RecordedTarget {
        fn apply_appearance(&self, appearance: UiAppearance) {
            self.appearance.set(Some(appearance));
        }
    }

    #[test]
    fn persisted_appearance_is_shared_by_main_and_onboarding_targets() {
        let settings = LocalSettings {
            theme: ThemeId::IrisMist,
            color_scheme: ColorSchemePreference::Dark,
            ..LocalSettings::default()
        };
        let main = RecordedTarget::default();
        let onboarding = RecordedTarget::default();
        let appearance = ui_appearance(&settings);

        apply(&main, appearance);
        apply(&onboarding, appearance);

        assert_eq!(Some(appearance), main.appearance.get());
        assert_eq!(Some(appearance), onboarding.appearance.get());
        assert_eq!(UiThemeId::IrisMist, appearance.theme);
        assert_eq!(UiColorSchemePreference::Dark, appearance.color_scheme);
    }
}
