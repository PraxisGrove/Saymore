# Workspace Organization

Goal: keep dependency direction clear, crate responsibilities focused, and
workspace configuration centralized.

## Saymore Layers

- `app`: business types, invariants, pure rules, use cases, and port traits.
- `infra`: concrete implementations for filesystem, database, HTTP, process, or
  other external capabilities.
- `desktop`: Slint entrypoint and dependency wiring.
- `xtask`: repository maintenance and packaging automation.

Hard constraints:

- `app` must not depend on `infra` or entrypoint crates.
- `desktop` may depend on both `app` and `infra` to wire dependencies.

## Root Cargo.toml

Use the workspace root to centralize metadata:

```toml
[workspace]
members = ["apps/desktop", "crates/app", "crates/infra", "crates/xtask"]
resolver = "2"

[workspace.package]
edition = "2024"
license-file = "LICENSE"
version = "0.1.0"
```

Member crates should inherit:

```toml
[package]
edition.workspace = true
license-file.workspace = true
version.workspace = true

[lints]
workspace = true
```

## Dependency Hygiene

- Use `[workspace.dependencies]` for shared dependency versions.
- Keep optional features near the crate that needs them.
- Run `cargo tree -d` when duplicate versions appear.
- Split crates when a dependency should not leak into the rest of the workspace.
