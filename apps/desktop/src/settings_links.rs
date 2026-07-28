use crate::{platform_open, ui::AppWindow};

const WEBSITE_URL: &str = "https://saymore.praxisgrove.org/";
const FEEDBACK_URL: &str = "https://github.com/PraxisGrove/Saymore/issues";
const TERMS_URL: &str = "https://github.com/PraxisGrove/Saymore/blob/main/LICENSE";
const PRIVACY_URL: &str = "https://github.com/PraxisGrove/Saymore/blob/main/README.zh-CN.md";
const RELEASES_URL: &str = "https://github.com/PraxisGrove/Saymore/releases/tag/v";

pub(crate) fn wire(ui: &AppWindow) {
    ui.on_open_website(|| open(WEBSITE_URL, "settings.website_open_failed"));
    ui.on_open_feedback(|| open(FEEDBACK_URL, "settings.feedback_open_failed"));
    ui.on_open_terms(|| open(TERMS_URL, "settings.terms_open_failed"));
    ui.on_open_privacy(|| open(PRIVACY_URL, "settings.privacy_open_failed"));
    ui.on_open_release_notes(|| {
        open(
            release_notes_url(env!("CARGO_PKG_VERSION")),
            "settings.release_notes_open_failed",
        );
    });
}

fn release_notes_url(version: &str) -> String {
    format!("{RELEASES_URL}{version}")
}

fn open(target: impl AsRef<std::ffi::OsStr>, failure_event: &str) {
    if let Err(error) = platform_open::open(target) {
        tracing::warn!(event = failure_event, reason = %error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_notes_link_targets_the_exact_version() {
        assert_eq!(
            "https://github.com/PraxisGrove/Saymore/releases/tag/v0.1.2",
            release_notes_url("0.1.2")
        );
    }

    #[test]
    fn support_links_target_the_published_product_resources() {
        assert_eq!("https://saymore.praxisgrove.org/", WEBSITE_URL);
        assert_eq!(
            "https://github.com/PraxisGrove/Saymore/issues",
            FEEDBACK_URL
        );
        assert_eq!(
            "https://github.com/PraxisGrove/Saymore/blob/main/LICENSE",
            TERMS_URL
        );
        assert_eq!(
            "https://github.com/PraxisGrove/Saymore/blob/main/README.zh-CN.md",
            PRIVACY_URL
        );
    }
}
