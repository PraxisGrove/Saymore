use std::{env, error::Error, fs::OpenOptions, io::Write, path::PathBuf};

const RELEASE_INTERVAL_MS: u64 = 48 * 60 * 60 * 1_000;

#[derive(Debug, PartialEq, Eq)]
struct ReleasePlan {
    should_release: bool,
    version: String,
    tag: String,
}

struct PlanInput<'a> {
    current_version: &'a str,
    latest_tag: Option<&'a str>,
    published_at_ms: Option<u64>,
    ahead_by: u64,
    now_ms: u64,
    force: bool,
}

pub(crate) fn run(args: &[String]) -> Result<(), Box<dyn Error>> {
    let output = match args {
        [flag, path] if flag == "--github-output" => PathBuf::from(path),
        _ => return Err("usage: release-plan --github-output <path>".into()),
    };
    let latest_tag = optional_env("SAYMORE_LATEST_TAG");
    let published_at_ms = optional_env("SAYMORE_LATEST_PUBLISHED_AT_MS")
        .map(|value| value.parse())
        .transpose()?;
    let ahead_by = env::var("SAYMORE_AHEAD_BY")?.parse()?;
    let now_ms = env::var("SAYMORE_NOW_MS")?.parse()?;
    let force = match env::var("SAYMORE_FORCE_RELEASE")?.as_str() {
        "true" => true,
        "false" => false,
        value => return Err(format!("invalid SAYMORE_FORCE_RELEASE value: {value}").into()),
    };
    let plan = plan(PlanInput {
        current_version: env!("CARGO_PKG_VERSION"),
        latest_tag: latest_tag.as_deref(),
        published_at_ms,
        ahead_by,
        now_ms,
        force,
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
    let due = match (input.latest_tag, input.published_at_ms) {
        (None, None) => true,
        (Some(_), Some(published_at_ms)) => {
            input.now_ms.saturating_sub(published_at_ms) >= RELEASE_INTERVAL_MS
        }
        (None, Some(_)) | (Some(_), None) => {
            return Err("latest tag and publication time must be provided together".into());
        }
    };
    let changed = input.latest_tag.is_none() || input.ahead_by > 0;
    let tag = format!("v{version}");
    Ok(ReleasePlan {
        should_release: input.force || (due && changed),
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
            published_at_ms: None,
            ahead_by: 0,
            now_ms: 1_000,
            force: false,
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
    fn due_release_increments_patch_when_commits_exist() {
        let actual = plan(PlanInput {
            current_version: "0.1.0",
            latest_tag: Some("v1.2.3"),
            published_at_ms: Some(1_000),
            ahead_by: 2,
            now_ms: 1_000 + RELEASE_INTERVAL_MS,
            force: false,
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
    fn cadence_and_unchanged_source_each_prevent_release() {
        let too_early = plan(PlanInput {
            current_version: "0.1.0",
            latest_tag: Some("v0.1.0"),
            published_at_ms: Some(1_000),
            ahead_by: 1,
            now_ms: 2_000,
            force: false,
        })
        .ok()
        .map(|plan| plan.should_release);
        let unchanged = plan(PlanInput {
            current_version: "0.1.0",
            latest_tag: Some("v0.1.0"),
            published_at_ms: Some(1_000),
            ahead_by: 0,
            now_ms: 1_000 + RELEASE_INTERVAL_MS,
            force: false,
        })
        .ok()
        .map(|plan| plan.should_release);
        assert_eq!(Some(false), too_early);
        assert_eq!(Some(false), unchanged);
    }

    #[test]
    fn force_overrides_cadence_and_change_checks() {
        let actual = plan(PlanInput {
            current_version: "0.1.0",
            latest_tag: Some("v0.1.0"),
            published_at_ms: Some(1_000),
            ahead_by: 0,
            now_ms: 2_000,
            force: true,
        })
        .ok()
        .map(|plan| plan.should_release);
        assert_eq!(Some(true), actual);
    }

    #[test]
    fn malformed_release_tags_are_rejected() {
        assert!(bump_tag("release-1.2.3").is_err());
        assert!(bump_tag("v1.2").is_err());
        assert!(bump_tag("v1.2.beta").is_err());
    }
}
