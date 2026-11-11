use std::{env, error::Error, fs::OpenOptions, io::Write, path::PathBuf};

#[derive(Debug, PartialEq, Eq)]
struct ReleasePlan {
    version: String,
    tag: String,
}

struct PlanInput<'a> {
    workspace_version: &'a str,
    requested_version: &'a str,
    latest_tag: &'a str,
}

pub(crate) fn run(args: &[String]) -> Result<(), Box<dyn Error>> {
    let output = match args {
        [flag, path] if flag == "--github-output" => PathBuf::from(path),
        _ => return Err("usage: release-plan --github-output <path>".into()),
    };
    let workspace_version = env!("CARGO_PKG_VERSION");
    let requested_version = env::var("SAYMORE_REQUESTED_VERSION")?;
    let latest_tag = env::var("SAYMORE_LATEST_TAG")?;
    let plan = plan(PlanInput {
        workspace_version,
        requested_version: &requested_version,
        latest_tag: &latest_tag,
    })?;

    let mut file = OpenOptions::new().create(true).append(true).open(output)?;
    writeln!(file, "version={}", plan.version)?;
    writeln!(file, "tag={}", plan.tag)?;
    Ok(())
}

fn plan(input: PlanInput<'_>) -> Result<ReleasePlan, Box<dyn Error>> {
    let workspace = parse_version(input.workspace_version)?;
    let requested = parse_version(input.requested_version)?;
    if workspace != requested {
        return Err(format!(
            "requested version {requested} does not match workspace version {workspace}; update the workspace version in a reviewed commit first"
        )
        .into());
    }
    if let Some(latest) = parse_tag(input.latest_tag)?
        && requested <= latest
    {
        return Err(format!(
            "workspace version {requested} must increase beyond latest release {latest}"
        )
        .into());
    }
    Ok(ReleasePlan {
        version: requested.to_string(),
        tag: format!("v{requested}"),
    })
}

fn parse_tag(tag: &str) -> Result<Option<Version>, Box<dyn Error>> {
    if tag.is_empty() {
        return Ok(None);
    }
    let version = tag
        .strip_prefix('v')
        .ok_or("latest release tag must start with v")?;
    parse_version(version).map(Some)
}

fn parse_version(value: &str) -> Result<Version, Box<dyn Error>> {
    let parts = value.split('.').collect::<Vec<_>>();
    match parts.as_slice() {
        [major, minor, patch]
            if [major, minor, patch]
                .iter()
                .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit())) =>
        {
            Ok(Version {
                major: major.parse()?,
                minor: minor.parse()?,
                patch: patch.parse()?,
            })
        }
        _ => Err(format!("version is not MAJOR.MINOR.PATCH: {value}").into()),
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct Version {
    major: u64,
    minor: u64,
    patch: u64,
}

impl std::fmt::Display for Version {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input<'a>(workspace_version: &'a str, latest_tag: &'a str) -> PlanInput<'a> {
        PlanInput {
            workspace_version,
            requested_version: workspace_version,
            latest_tag,
        }
    }

    #[test]
    fn explicit_workspace_version_creates_the_release_tag() {
        assert_eq!(
            plan(input("0.2.5", "v0.2.4")).ok(),
            Some(ReleasePlan {
                version: "0.2.5".to_owned(),
                tag: "v0.2.5".to_owned(),
            })
        );
    }

    #[test]
    fn higher_patch_and_minor_versions_are_accepted() {
        for version in ["0.2.5", "0.3.0", "1.0.0"] {
            assert!(plan(input(version, "v0.2.4")).is_ok());
        }
    }

    #[test]
    fn first_release_uses_the_reviewed_workspace_version() {
        assert!(plan(input("0.2.5", "")).is_ok());
    }

    #[test]
    fn requested_version_must_match_the_reviewed_workspace() {
        let mut input = input("0.2.5", "v0.2.4");
        input.requested_version = "0.2.6";
        assert!(plan(input).is_err());
    }

    #[test]
    fn unchanged_workspace_does_not_implicitly_create_a_patch_release() {
        assert!(plan(input("0.2.4", "v0.2.4")).is_err());
    }

    #[test]
    fn older_workspace_cannot_move_latest_backwards() {
        assert!(plan(input("0.2.5", "v1.2.3")).is_err());
    }

    #[test]
    fn malformed_release_tags_are_rejected() {
        assert!(parse_tag("release-1.2.3").is_err());
        assert!(parse_tag("v1.2").is_err());
        assert!(parse_tag("v1.2.beta").is_err());
    }
}
