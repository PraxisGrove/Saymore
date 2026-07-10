# Rust Template

A Rust workspace template optimized for human and AI-assisted development:
clear crate boundaries, mature Rust quality gates, small public APIs, and
explicit contributor instructions.

This is not an AI application framework. It is a general Rust template designed
to keep projects easy for humans and coding agents to understand, modify,
verify, and review.

## Structure

```text
crates/
  domain/  # business types and rules
  app/     # use cases and ports
  infra/   # concrete implementations for app ports
  cli/     # binary entrypoint and dependency wiring
  xtask/   # Rust-only project maintenance tasks
```

The intended dependency direction is:

```text
cli -> app -> domain
cli -> infra -> app
```

Keep `domain` free of infrastructure and entrypoint concerns. Put traits that
describe required capabilities near the use cases that consume them.

## Tooling Policy

The required development path uses Cargo plus the mature-project gate tools
`cargo-nextest` and `cargo-deny`:

```bash
cargo fmt
cargo check
cargo nextest run
cargo test --doc
cargo clippy
cargo deny check
cargo build
```

Extra tools such as `just`, `prek`, or release helpers are optional. They can
improve local workflow, but CI and handoff verification should use the required
gate below.

## Template Scope

This repository is the template. It does not maintain a separate `template/`
copy of itself.

The current base is a dependency-light Rust workspace. When a project needs a
specific application shape, evolve this workspace directly toward one of the
documented application types:

- server plus full-stack frontend
- server plus desktop client
- desktop-only app
- server-only service

## Development

Run the full local gate with Cargo:

```bash
cargo fmt --all --check
cargo check --workspace --all-targets
cargo nextest run --workspace --all-targets
cargo test --workspace --doc
cargo clippy --workspace --all-targets -- -D warnings
cargo deny check
cargo build --workspace --all-targets --release
cargo run -p xtask -- size
```

When you want Cargo to format the code:

```bash
cargo fmt --all
```

Run the sample CLI:

```bash
cargo run -p template-cli -- workspace
```

## Workspace Conventions

Shared package metadata, dependency versions, lints, and build profiles live in
the root `Cargo.toml`. Crates should inherit them:

```toml
edition.workspace = true
license.workspace = true

[lints]
workspace = true
```

Prefer declaring third-party dependencies under `[workspace.dependencies]` and
using `workspace = true` from member crates. This keeps versions and feature
choices visible in one place.

## AI-Assisted Development

Project-level instructions for coding agents live in `AGENTS.md`. Human-facing
engineering guidance lives under `docs/`:

- `docs/architecture.md`
- `docs/development.md`
- `docs/technology-stack.md`
- `docs/error-handling.md`
- `docs/fail-fast.md`
- `docs/dependency-policy.md`
- `docs/observability.md`
- `docs/testing.md`
- `docs/review.md`
- `docs/application-types.md`

These files are part of the template contract. Keep them current when changing
crate layout, required gates, or review policy.

Production code should fail early with explicit errors and validated types, not
late with assertions or panics. Tests may still use assertions to verify
behavior.

## Optional `just` Shortcuts

`just` is not required. If it is installed, the included `justfile` provides
short aliases for the required gate commands:

```bash
just ci
just test
just test-doc
just clippy
just deny
just size
just fmt-fix
```

CI and project documentation should continue to spell out the underlying
commands so the required gates remain explicit.

## Tests

Run fast Rust tests with nextest:

```bash
cargo nextest run --workspace --all-targets
cargo test --workspace --doc
```

Add end-to-end tests only when the project needs them. Keep the base template
Rust-only; project-specific e2e tooling should be introduced deliberately.
