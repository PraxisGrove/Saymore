use std::{collections::HashMap, path::Path};

fn main() {
    #[cfg(target_os = "windows")]
    if let Err(error) = embed_windows_resources() {
        eprintln!("failed to embed Windows resources: {error}");
        std::process::exit(1);
    }
    println!("cargo:rerun-if-changed=i18n");
    if let Err(error) = validate_translations(
        Path::new("i18n/saymore-desktop.pot"),
        Path::new("i18n/zh-Hans/LC_MESSAGES/saymore-desktop.po"),
    ) {
        eprintln!("invalid Saymore translations: {error}");
        std::process::exit(1);
    }
    let config = slint_build::CompilerConfiguration::new().with_bundled_translations("i18n");
    if let Err(error) = slint_build::compile_with_config("ui/app-window.slint", config) {
        eprintln!("failed to compile Saymore UI: {error}");
        std::process::exit(1);
    }
}

#[cfg(target_os = "windows")]
fn embed_windows_resources() -> Result<(), std::io::Error> {
    println!("cargo:rerun-if-changed=icons/taskbar.ico");
    winresource::WindowsResource::new()
        .set_icon("icons/taskbar.ico")
        .set_icon_with_id("icons/taskbar.ico", "2")
        .compile()
}

fn validate_translations(template_path: &Path, translation_path: &Path) -> Result<(), String> {
    let template = rspolib::pofile(template_path).map_err(|error| error.to_string())?;
    let translation = rspolib::pofile(translation_path).map_err(|error| error.to_string())?;
    let entries = translation
        .entries
        .iter()
        .filter(|entry| !entry.obsolete)
        .map(|entry| ((entry.msgctxt.as_deref(), entry.msgid.as_str()), entry))
        .collect::<HashMap<_, _>>();

    let mut problems = Vec::new();
    for source in template.entries.iter().filter(|entry| !entry.obsolete) {
        let context = source.msgctxt.as_deref().unwrap_or_default();
        let Some(translated) = entries.get(&(source.msgctxt.as_deref(), source.msgid.as_str()))
        else {
            problems.push(format!("missing key {context:?}"));
            continue;
        };
        if translated.fuzzy() {
            problems.push(format!("fuzzy key {context:?}"));
            continue;
        }
        let expected_placeholders = placeholder_count(&source.msgid);
        if source.msgid_plural.is_some() {
            if translated.msgstr_plural.is_empty()
                || translated.msgstr_plural.iter().any(String::is_empty)
            {
                problems.push(format!("untranslated plural key {context:?}"));
                continue;
            }
            if translated
                .msgstr_plural
                .iter()
                .any(|message| placeholder_count(message) != expected_placeholders)
            {
                problems.push(format!("placeholder mismatch for plural key {context:?}"));
            }
        } else if let Some(message) = translated
            .msgstr
            .as_deref()
            .filter(|message| !message.is_empty())
        {
            if placeholder_count(message) != expected_placeholders {
                problems.push(format!("placeholder mismatch for key {context:?}"));
            }
        } else {
            problems.push(format!("untranslated key {context:?}"));
        }
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join(", "))
    }
}

fn placeholder_count(message: &str) -> usize {
    message.match_indices("{}").count()
}
