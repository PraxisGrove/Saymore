# Dependency Policy

Dependencies are architecture decisions. Add them deliberately.

## Before Adding A Dependency

Document:

- What problem the dependency solves.
- Why the standard library or an existing dependency is not enough.
- Maintenance status and release activity.
- License compatibility.
- Feature flags being enabled.
- Alternatives considered.

## Workspace Rules

- Put shared versions in `[workspace.dependencies]`.
- Member crates should use `workspace = true`.
- Keep heavy dependencies away from `domain`.
- Prefer dependency boundaries at crate edges.
- Run `cargo tree -d` when duplicate versions appear.

## Required Dependency Gate

Run `cargo deny check` before handing off a change. The repository-level
`deny.toml` defines the license, advisory, duplicate-version, and wildcard
dependency policy.

Required template tasks should be implemented in Rust under `crates/xtask`.
