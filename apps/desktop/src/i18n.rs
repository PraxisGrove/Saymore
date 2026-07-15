use std::{
    sync::{Arc, Mutex},
    thread,
};

use slint::{ComponentHandle, SharedString};
use template_app::{LocalSettingsStore, UiLanguagePreference};
use template_infra::SqliteStorage;

use crate::ui::{AppWindow, Translations, UiLanguage};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectiveLanguage {
    English,
    SimplifiedChinese,
}

impl EffectiveLanguage {
    const fn slint_language(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::SimplifiedChinese => "zh-Hans",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LanguageContext {
    system_language: EffectiveLanguage,
}

pub fn initialize(
    ui: &AppWindow,
    preference: UiLanguagePreference,
) -> Result<LanguageContext, slint::SelectBundledTranslationError> {
    let system_language = resolve_system_language(sys_locale::get_locales());
    let effective = effective_language(preference, system_language);
    slint::select_bundled_translation(effective.slint_language())?;
    ui.set_ui_language(ui_preference(preference));
    ui.set_system_ui_language(ui_effective_language(system_language));
    Ok(LanguageContext { system_language })
}

pub fn wire(
    ui: &AppWindow,
    storage: Arc<SqliteStorage>,
    settings_guard: Arc<Mutex<()>>,
    context: LanguageContext,
) {
    let weak_ui = ui.as_weak();
    ui.on_set_ui_language(move |language| {
        let Some(ui) = weak_ui.upgrade() else {
            return;
        };
        let preference = preference_from_ui(language);
        ui.set_ui_language_saving(true);
        ui.set_ui_language_status(SharedString::new());

        let save_ui = ui.as_weak();
        let save_storage = Arc::clone(&storage);
        let save_guard = Arc::clone(&settings_guard);
        let spawn_result = thread::Builder::new()
            .name("saymore-save-ui-language".to_owned())
            .spawn(move || {
                let result = save_guard
                    .lock()
                    .map_err(|_| "settings update lock was poisoned".to_owned())
                    .and_then(|_guard| {
                        let mut settings = save_storage
                            .load_settings()
                            .map_err(|error| error.to_string())?;
                        settings.ui_language = preference;
                        save_storage
                            .save_settings(settings)
                            .map_err(|error| error.to_string())
                    });
                let _ = save_ui.upgrade_in_event_loop(move |ui| {
                    finish_language_change(&ui, preference, context, result);
                });
            });

        if spawn_result.is_err() {
            finish_language_change(
                &ui,
                preference,
                context,
                Err("failed to start settings worker".to_owned()),
            );
        }
    });
}

fn finish_language_change(
    ui: &AppWindow,
    preference: UiLanguagePreference,
    context: LanguageContext,
    result: Result<(), String>,
) {
    ui.set_ui_language_saving(false);
    match result {
        Ok(()) => {
            let effective = effective_language(preference, context.system_language);
            if let Err(error) = slint::select_bundled_translation(effective.slint_language()) {
                tracing::error!(event = "i18n.language_select_failed", reason = %error);
                apply_save_error(ui);
                return;
            }
            ui.set_ui_language(ui_preference(preference));
            ui.set_ui_language_status(SharedString::new());
            ui.invoke_refresh_history();
            ui.invoke_refresh_dictionary();
            ui.invoke_refresh_usage();
            ui.invoke_refresh_localized_state();
        }
        Err(error) => {
            tracing::warn!(event = "i18n.language_save_failed", reason = %error);
            apply_save_error(ui);
        }
    }
}

fn apply_save_error(ui: &AppWindow) {
    let message = ui.global::<Translations>().get_language_save_failed();
    ui.set_ui_language_status(message);
}

fn effective_language(
    preference: UiLanguagePreference,
    system_language: EffectiveLanguage,
) -> EffectiveLanguage {
    match preference {
        UiLanguagePreference::System => system_language,
        UiLanguagePreference::English => EffectiveLanguage::English,
        UiLanguagePreference::SimplifiedChinese => EffectiveLanguage::SimplifiedChinese,
    }
}

fn resolve_system_language(locales: impl IntoIterator<Item = String>) -> EffectiveLanguage {
    locales
        .into_iter()
        .find_map(|locale| supported_language(&locale))
        .unwrap_or(EffectiveLanguage::English)
}

fn supported_language(locale: &str) -> Option<EffectiveLanguage> {
    let normalized = locale.replace('_', "-").to_ascii_lowercase();
    let subtags = normalized.split('-').collect::<Vec<_>>();
    match subtags.first().copied() {
        Some("en") => Some(EffectiveLanguage::English),
        Some("zh")
            if subtags
                .iter()
                .any(|part| matches!(*part, "hant" | "tw" | "hk" | "mo")) =>
        {
            None
        }
        Some("zh") => Some(EffectiveLanguage::SimplifiedChinese),
        _ => None,
    }
}

fn ui_preference(preference: UiLanguagePreference) -> UiLanguage {
    match preference {
        UiLanguagePreference::System => UiLanguage::System,
        UiLanguagePreference::English => UiLanguage::English,
        UiLanguagePreference::SimplifiedChinese => UiLanguage::SimplifiedChinese,
    }
}

fn preference_from_ui(language: UiLanguage) -> UiLanguagePreference {
    match language {
        UiLanguage::System => UiLanguagePreference::System,
        UiLanguage::English => UiLanguagePreference::English,
        UiLanguage::SimplifiedChinese => UiLanguagePreference::SimplifiedChinese,
    }
}

fn ui_effective_language(language: EffectiveLanguage) -> UiLanguage {
    match language {
        EffectiveLanguage::English => UiLanguage::English,
        EffectiveLanguage::SimplifiedChinese => UiLanguage::SimplifiedChinese,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_the_first_supported_system_language() {
        assert_eq!(
            EffectiveLanguage::SimplifiedChinese,
            resolve_system_language([
                "fr-FR".to_owned(),
                "zh-Hans-CN".to_owned(),
                "en-US".to_owned()
            ])
        );
        assert_eq!(
            EffectiveLanguage::English,
            resolve_system_language(["fr-FR".to_owned(), "en-GB".to_owned(), "zh-CN".to_owned()])
        );
    }

    #[test]
    fn traditional_chinese_does_not_fall_back_to_simplified_chinese() {
        assert_eq!(
            EffectiveLanguage::English,
            resolve_system_language(["zh-Hant-TW".to_owned(), "fr-FR".to_owned()])
        );
        assert_eq!(
            EffectiveLanguage::English,
            resolve_system_language(["zh-HK".to_owned(), "en-US".to_owned()])
        );
    }

    #[test]
    fn explicit_preference_overrides_the_system_language() {
        assert_eq!(
            EffectiveLanguage::English,
            effective_language(
                UiLanguagePreference::English,
                EffectiveLanguage::SimplifiedChinese
            )
        );
        assert_eq!(
            EffectiveLanguage::SimplifiedChinese,
            effective_language(
                UiLanguagePreference::SimplifiedChinese,
                EffectiveLanguage::English
            )
        );
    }
}
