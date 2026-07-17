use std::{error::Error, path::Path};

use template_infra::AppEnvironment;

const ENVIRONMENT_FLAG: &str = "--environment";
const AUTOSTART_FLAG: &str = "--autostart";
const DEVELOPMENT_MARKER: &str = "saymore-development-environment";

pub fn resolve() -> Result<AppEnvironment, Box<dyn Error>> {
    if bundled_environment(&std::env::current_exe()?) == Some(AppEnvironment::Development) {
        return Ok(AppEnvironment::Development);
    }
    resolve_from(std::env::args().skip(1), default_for_build())
}

pub fn started_automatically() -> bool {
    std::env::args()
        .skip(1)
        .any(|argument| argument == AUTOSTART_FLAG)
}

fn bundled_environment(executable: &Path) -> Option<AppEnvironment> {
    executable
        .parent()
        .and_then(Path::parent)
        .map(|contents| contents.join("Resources").join(DEVELOPMENT_MARKER))
        .filter(|marker| marker.is_file())
        .map(|_| AppEnvironment::Development)
}

fn default_for_build() -> AppEnvironment {
    if cfg!(debug_assertions) {
        AppEnvironment::Development
    } else {
        AppEnvironment::Production
    }
}

fn resolve_from(
    mut arguments: impl Iterator<Item = String>,
    default: AppEnvironment,
) -> Result<AppEnvironment, Box<dyn Error>> {
    let mut selected = None;
    while let Some(argument) = arguments.next() {
        if argument != ENVIRONMENT_FLAG {
            continue;
        }
        if selected.is_some() {
            return Err("--environment may only be provided once".into());
        }
        let value = arguments
            .next()
            .ok_or("--environment requires production or development")?;
        selected = Some(match value.as_str() {
            "production" => AppEnvironment::Production,
            "development" => AppEnvironment::Development,
            _ => return Err(format!("unsupported Saymore environment: {value}").into()),
        });
    }
    Ok(selected.unwrap_or(default))
}

#[cfg(test)]
mod tests {
    use std::{fs, time::SystemTime};

    use super::*;

    #[test]
    fn uses_the_build_default_without_an_override() {
        assert_eq!(
            Ok(AppEnvironment::Development),
            resolve_from(std::iter::empty(), AppEnvironment::Development)
                .map_err(|error| error.to_string())
        );
    }

    #[test]
    fn explicit_environment_overrides_the_build_default() {
        assert_eq!(
            Ok(AppEnvironment::Development),
            resolve_from(
                [ENVIRONMENT_FLAG.to_owned(), "development".to_owned()].into_iter(),
                AppEnvironment::Production,
            )
            .map_err(|error| error.to_string())
        );
    }

    #[test]
    fn rejects_missing_unknown_and_duplicate_values() {
        assert!(
            resolve_from(
                [ENVIRONMENT_FLAG.to_owned()].into_iter(),
                AppEnvironment::Production,
            )
            .is_err()
        );
        assert!(
            resolve_from(
                [ENVIRONMENT_FLAG.to_owned(), "staging".to_owned()].into_iter(),
                AppEnvironment::Production,
            )
            .is_err()
        );
        assert!(
            resolve_from(
                [
                    ENVIRONMENT_FLAG.to_owned(),
                    "development".to_owned(),
                    ENVIRONMENT_FLAG.to_owned(),
                    "production".to_owned(),
                ]
                .into_iter(),
                AppEnvironment::Production,
            )
            .is_err()
        );
    }

    #[test]
    fn environment_parser_ignores_the_autostart_marker() {
        assert_eq!(
            Ok(AppEnvironment::Production),
            resolve_from(
                [AUTOSTART_FLAG.to_owned()].into_iter(),
                AppEnvironment::Production,
            )
            .map_err(|error| error.to_string())
        );
    }

    #[test]
    fn preview_bundle_marker_forces_the_development_environment() -> Result<(), Box<dyn Error>> {
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)?
            .as_nanos();
        let bundle = std::env::temp_dir().join(format!("saymore-bundle-{nonce}"));
        let executable = bundle.join("Contents/MacOS/saymore-desktop");
        let resources = bundle.join("Contents/Resources");
        fs::create_dir_all(&resources)?;
        fs::write(resources.join(DEVELOPMENT_MARKER), [])?;

        let environment = bundled_environment(&executable);

        fs::remove_dir_all(bundle)?;
        if environment == Some(AppEnvironment::Development) {
            Ok(())
        } else {
            Err("Preview bundle marker did not select Development".into())
        }
    }
}
