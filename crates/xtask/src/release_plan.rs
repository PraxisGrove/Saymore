use std::{env, error::Error, fs::OpenOptions, io::Write, path::PathBuf};

#[derive(Debug, PartialEq, Eq)]
struct ReleasePlan {
    should_release: bool,
    version: String,
    tag: String,
}

struct PlanInput<'a> {
    current_version: &'a str,
    latest_tag: Option<&'a str>,
    ahead_by: u64,
}

pub(crate) fn run(args: &[String]) -> Result<(), Box<dyn Error>> {
    let output = match args {
        [flag, path] if flag == "--github-output" => PathBuf::from(path),
        _ => return Err("usage: release-plan --github-output <path>".into()),
    };
    let latest_tag = optional_env("SAYMORE_LATEST_TAG");
    let ahead_by = env::var("SAYMORE_AHEAD_BY")?.parse()?;
    let plan = plan(PlanInput {
        current_version: env!("CARGO_PKG_VERSION"),
        latest_tag: latest_tag.as_deref(),
        ahead_by,
    })?;

    let mut file = OpenOptions::new().create(true).append(true).open(output)?;
    writeln!(file, "should-release={}", plan.should_release)?;
    writeln!(file, "version={}", plan.version)?;
    writeln!(file, "tag={}", plan.tag)?;
    Ok(())
}

fn optional_env(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.is_empty())
}

fn plan(input: PlanInput<'_>) -> Result<ReleasePlan, Box<dyn Error>> {
    let version = match input.latest_tag {
        Some(tag) => bump_tag(tag)?,
        None => parse_version(input.current_version)?.to_string(),
    };
    let changed = input.latest_tag.is_none() || input.ahead_by > 0;
    let tag = format!("v{version}");
    Ok(ReleasePlan {
        should_release: changed,
        version,
        tag,
    })
}

fn bump_tag(tag: &str) -> Result<String, Box<dyn Error>> {
    let version = tag
        .strip_prefix('v')
        .ok_or("latest release tag must start with v")?;
    let mut version = parse_version(version)?;
    version.patch = version
        .patch
        .checked_add(1)
        .ok_or("patch version overflow")?;
    Ok(version.to_string())
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

    #[test]
    fn first_release_uses_workspace_version() {
        let actual = plan(PlanInput {
            current_version: "0.1.0",
            latest_tag: None,
            ahead_by: 0,
        })
        .ok();
        assert_eq!(
            Some(ReleasePlan {
                should_release: true,
                version: "0.1.0".to_owned(),
                tag: "v0.1.0".to_owned(),
            }),
            actual
        );
    }

    #[test]
    fn release_increments_patch_when_commits_exist() {
        let actual = plan(PlanInput {
            current_version: "0.1.0",
            latest_tag: Some("v1.2.3"),
            ahead_by: 2,
        })
        .ok();
        assert_eq!(
            Some(ReleasePlan {
                should_release: true,
                version: "1.2.4".to_owned(),
                tag: "v1.2.4".to_owned(),
            }),
            actual
        );
    }

    #[test]
    fn unchanged_source_prevents_release() {
        let unchanged = plan(PlanInput {
            current_version: "0.1.0",
            latest_tag: Some("v0.1.0"),
            ahead_by: 0,
        })
        .ok()
        .map(|plan| plan.should_release);
        assert_eq!(Some(false), unchanged);
    }

    #[test]
    fn malformed_release_tags_are_rejected() {
        assert!(bump_tag("release-1.2.3").is_err());
        assert!(bump_tag("v1.2").is_err());
        assert!(bump_tag("v1.2.beta").is_err());
    }
}
