# Saymore

Saymore is a local-first, provider-agnostic voice input application for macOS
and Windows. It records speech from a global trigger, recognizes it, optionally
normalizes and refines the transcript, and delivers final text to the current
input position.

The production desktop stack is Rust with Slint. See `docs/technology-stack.md`
and `docs/adr/0003-use-slint-for-the-desktop-ui.md` for the accepted decision.

[![Made with Slint](https://raw.githubusercontent.com/slint-ui/slint/master/logo/MadeWithSlint-logo-whitebg.png)](https://slint.dev/)

## Structure

```text
crates/
  app/     # business rules, use cases, and ports
  infra/   # concrete implementations for app ports
  xtask/   # Rust-only project maintenance tasks
apps/
  desktop/ # Slint desktop entrypoint and UI
```

The intended dependency direction is:

```text
desktop -> app
desktop -> infra -> app
```

Keep business rules and port traits in `app`, concrete adapters in `infra`, and
Slint plus process wiring in `desktop`.

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
improve local workflow, but CI and pre-push verification should use the required
gate below.

## Product Scope

The MVP is a desktop-only application with no hosted Saymore backend. Product
scope, platform order, provider boundaries, and vertical slices live in
`docs/product/open-source-voice-input-wayfinder.md`.

## License

Saymore is source-available under the [PolyForm Shield License 1.0.0](LICENSE).
Personal, internal organizational, and other noncompeting uses are permitted.
Providing a product or service that competes with Saymore requires a separate
commercial license from the maintainers. This is a source-available license, not
an OSI-approved open-source license. Third-party assets retain their own
licenses.

## Development

Run focused checks while developing. Immediately before `git push`, run the full
local gate with Cargo:

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

## Workspace Conventions

Shared package metadata, dependency versions, lints, and build profiles live in
the root `Cargo.toml`. Crates should inherit them:

```toml
edition.workspace = true
license-file.workspace = true

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
- `docs/releasing.md`
- `docs/adr/0005-use-polyform-shield-source-available-license.md`
- `docs/technology-stack.md`
- `docs/error-handling.md`
- `docs/fail-fast.md`
- `docs/dependency-policy.md`
- `docs/observability.md`
- `docs/testing.md`
- `docs/review.md`
- `docs/application-types.md`

These files are part of the project contract. Keep them current when changing
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

Add end-to-end tests only when the project needs them. Project-specific e2e
tooling should be introduced deliberately.
