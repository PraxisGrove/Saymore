use template_app::RefinementStrength;

use super::*;
use crate::ui::PolishingStrength;

pub(super) fn wire_polishing_strength(ui: &AppWindow, settings: LocalSettingsHandle) {
    let strength_ui = ui.as_weak();
    ui.on_set_polishing_strength(move |strength| {
        set_polishing_strength(strength_ui.clone(), settings.clone(), strength);
    });
}

pub(super) fn ui_strength(strength: RefinementStrength) -> PolishingStrength {
    match strength {
        RefinementStrength::Light => PolishingStrength::Light,
        RefinementStrength::Standard => PolishingStrength::Standard,
        RefinementStrength::Thorough => PolishingStrength::Thorough,
    }
}

fn refinement_strength(strength: PolishingStrength) -> RefinementStrength {
    match strength {
        PolishingStrength::Light => RefinementStrength::Light,
        PolishingStrength::Standard => RefinementStrength::Standard,
        PolishingStrength::Thorough => RefinementStrength::Thorough,
    }
}

fn set_polishing_strength(
    ui: slint::Weak<AppWindow>,
    settings: LocalSettingsHandle,
    strength: PolishingStrength,
) {
    let Some(window) = ui.upgrade() else {
        return;
    };
    let previous = window.get_polishing_strength();
    window.set_polishing_strength(strength);
    window.set_polishing_strength_status(SharedString::new());
    let failure_ui = ui.clone();
    let result = settings.submit(
        LocalSettingsChange::SetRefinementStrength(refinement_strength(strength)),
        move |result| {
            if let Some(window) = ui.upgrade()
                && let Err(error) = result
            {
                tracing::warn!(event = "settings.polishing_strength_save_failed", reason = %error);
                window.set_polishing_strength(previous);
                window.set_polishing_strength_status(
                    window.global::<Translations>().get_settings_save_failed(),
                );
            }
        },
    );
    if let Err(error) = result
        && let Some(window) = failure_ui.upgrade()
    {
        tracing::warn!(event = "settings.polishing_strength_submit_failed", reason = %error);
        window.set_polishing_strength(previous);
        window.set_polishing_strength_status(
            window.global::<Translations>().get_settings_save_failed(),
        );
    }
}
