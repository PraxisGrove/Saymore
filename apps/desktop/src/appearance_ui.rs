use slint::{ComponentHandle, SharedString};
use template_app::{ColorSchemePreference, LocalSettings, LocalSettingsChange, ThemeId};

use crate::{
    local_settings_runtime::LocalSettingsHandle,
    ui::{
        AppColors, AppWindow, ColorSchemePreference as UiColorSchemePreference,
        ThemeId as UiThemeId, Translations,
    },
};

pub fn wire(ui: &AppWindow, initial: &LocalSettings, settings: LocalSettingsHandle) {
    apply(ui, initial);
    wire_theme(ui, settings.clone());
    wire_color_scheme(ui, settings);
}

fn wire_theme(ui: &AppWindow, settings: LocalSettingsHandle) {
    let weak = ui.as_weak();
    ui.on_set_theme(move |theme| {
        let completion_ui = weak.clone();
        let failure_ui = weak.clone();
        let result = settings.submit(
            LocalSettingsChange::SetTheme(theme_from_ui(theme)),
            move |result| match result {
                Ok(committed) => apply_to_weak(&completion_ui, &committed),
                Err(error) => apply_error(&completion_ui, "theme", error),
            },
        );
        if let Err(error) = result {
            apply_error(&failure_ui, "theme", error);
        }
    });
}

fn wire_color_scheme(ui: &AppWindow, settings: LocalSettingsHandle) {
    let weak = ui.as_weak();
    ui.on_set_color_scheme(move |scheme| {
        let completion_ui = weak.clone();
        let failure_ui = weak.clone();
        let result = settings.submit(
            LocalSettingsChange::SetColorScheme(color_scheme_from_ui(scheme)),
            move |result| match result {
                Ok(committed) => apply_to_weak(&completion_ui, &committed),
                Err(error) => apply_error(&completion_ui, "color_scheme", error),
            },
        );
        if let Err(error) = result {
            apply_error(&failure_ui, "color_scheme", error);
        }
    });
}

fn apply_to_weak(ui: &slint::Weak<AppWindow>, settings: &LocalSettings) {
    if let Some(ui) = ui.upgrade() {
        apply(&ui, settings);
    }
}

fn apply(ui: &AppWindow, settings: &LocalSettings) {
    let theme = theme_to_ui(settings.theme);
    let color_scheme = color_scheme_to_ui(settings.color_scheme);
    ui.set_theme_id(theme);
    ui.set_color_scheme(color_scheme);
    ui.set_appearance_status(SharedString::new());
    ui.global::<AppColors>().set_theme_id(theme);
    ui.global::<AppColors>().set_color_scheme(color_scheme);
    crate::main_window::schedule_titlebar_integration(ui);
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
        ThemeId::WarmClay => UiThemeId::WarmClay,
        ThemeId::LimePulse => UiThemeId::LimePulse,
        ThemeId::BerryGraphite => UiThemeId::BerryGraphite,
        ThemeId::IrisMist => UiThemeId::IrisMist,
        ThemeId::ClearSky => UiThemeId::ClearSky,
    }
}

fn theme_from_ui(theme: UiThemeId) -> ThemeId {
    match theme {
        UiThemeId::WarmClay => ThemeId::WarmClay,
        UiThemeId::LimePulse => ThemeId::LimePulse,
        UiThemeId::BerryGraphite => ThemeId::BerryGraphite,
        UiThemeId::IrisMist => ThemeId::IrisMist,
        UiThemeId::ClearSky => ThemeId::ClearSky,
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
