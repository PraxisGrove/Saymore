# Workspace Organization

Goal: keep dependency direction clear, crate responsibilities focused, and
workspace configuration centralized.

## Recommended Layers

- `domain`: business types, invariants, and pure rules.
- `app`: use cases and port traits.
- `infra`: concrete implementations for filesystem, database, HTTP, process, or
  other external capabilities.
- `cli` or `adapters-*`: entrypoints and dependency wiring.

Hard constraints:

- `domain` must not depend on `infra` or entrypoint crates.
- `app` must not depend on entrypoint crates.
- Entry points may depend on both `app` and `infra` to wire dependencies.

## Root Cargo.toml

Use the workspace root to centralize metadata:

```toml
[workspace]
members = ["crates/domain", "crates/app", "crates/infra", "crates/cli"]
resolver = "2"

[workspace.package]
edition = "2024"
license = "MIT OR Apache-2.0"
version = "0.1.0"

[workspace.dependencies]
anyhow = "1"
```

Member crates should inherit:

```toml
[package]
edition.workspace = true
license.workspace = true
version.workspace = true

[lints]
workspace = true
```

## Dependency Hygiene

- Use `[workspace.dependencies]` for shared dependency versions.
- Keep optional features near the crate that needs them.
- Run `cargo tree -d` when duplicate versions appear.
- Split crates when a dependency should not leak into the rest of the
  workspace.
