# AGENTS.md

This repository is a Rust workspace template optimized for AI-assisted
development. It is not an AI application framework. Keep the template useful
for ordinary Rust projects while making it easy for humans and coding agents to
understand, modify, verify, and review changes.

## Required Workflow

Use standard Cargo commands as the source of truth:

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

`cargo-nextest` and `cargo-deny` are required for the mature-project gate.
`just`, `prek`, and release helpers are optional conveniences.

## Architecture Rules

- Keep crate boundaries clear: `domain`, `app`, `infra`, and `cli` have distinct
  responsibilities.
- Keep `domain` free of infrastructure, entrypoint, filesystem, network, and
  process concerns.
- Put use cases and port traits in `app`; put concrete implementations in
  `infra`.
- Keep `cli` focused on input parsing, dependency wiring, and process exit
  behavior.
- Prefer adding a focused module or crate over growing a central catch-all file.
- Keep public crate APIs small. Export intent, not implementation details.

## Rust Style

- Use inline format arguments when possible: `format!("{name}")`.
- Avoid boolean or ambiguous `Option` positional parameters when they make
  callsites hard to read. Prefer enums, named constructors, or small value types.
- Production code must not use `unwrap`, `expect`, `panic`, `todo`, or
  `unimplemented`; return explicit errors or reject invalid state at
  construction/parsing boundaries.
- Do not use production assertions as validation. Tests may use assertions to
  verify behavior.
- Prefer exhaustive `match` statements over wildcard arms when the domain is
  closed and meaningful.
- Newly added traits must include doc comments explaining their role and what
  implementations are expected to provide.
- Do not add one-off helper functions that are referenced only once unless they
  isolate genuinely complex logic.

## Size Limits

- Target Rust source files under 500 lines, excluding tests.
- Files over 600 lines are warnings and should have a split plan.
- Files over 800 lines should be split before adding more behavior.
- Functions over 80 lines are warnings.
- Functions over 150 lines should be split unless there is a documented reason.

## Testing Rules

- Test behavior and contracts, not implementation details.
- Prefer comparing complete values over asserting field by field.
- Do not add tests for static constants or for logic that was removed.
- Do not expose production APIs only to make tests easier.
- Put integration tests in the owning crate's `tests/` directory.
- Move shared test helpers into a dedicated test module or test-support crate
  once duplication becomes meaningful.

## Review Rules

- Keep non-mechanical changes under roughly 500 changed lines when possible.
- Split changes over 800 lines into reviewable stages unless the diff is purely
  mechanical.
- Public API changes must explain expected callers and migration impact.
- Dependency changes must explain why the dependency is needed.
- Generated code and handwritten code should be separated clearly.

## Technology Choices

- Keep the base template dependency-light.
- Prefer `thiserror` for library errors and `anyhow` for binary/xtask boundary
  errors.
- Prefer `tracing` for observability once runtime diagnostics are needed.
- Prefer `tokio` for async Rust and `axum` for new HTTP services when a project
  actually needs a web framework.
- Required template automation belongs in Rust under `crates/xtask`.
