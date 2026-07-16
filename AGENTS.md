# AGENTS.md

This repository contains the Saymore Rust desktop application. Keep it easy for
humans and coding agents to understand, modify, verify, and review.

## Verification Workflow

During implementation, run only the checks that are relevant to the code being
changed, such as a focused test target or `cargo check` for the owning crate.
Do not run the full workspace gate or a dual-axis code review merely because an
ordinary task is ending.

## Pre-Push Workflow

Immediately before any `git push`, run the full workspace gate below. Use
standard Cargo commands as the source of truth:

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

After the gate passes, review the complete change being pushed along two axes:

- Standards: conformance with the repository's documented engineering rules.
- Spec: conformance with the originating request, issue, PRD, or specification.

Resolve blocking findings before pushing. If review fixes materially change the
code, rerun the affected checks; rerun the full gate when the changes are broad.

## Architecture Rules

- Keep crate boundaries clear: `app`, `infra`, `desktop`, and `xtask` have
  distinct responsibilities.
- Put business rules, use cases, and port traits in `app`; keep it free of UI,
  filesystem, network, and process implementations.
- Put concrete implementations in `infra`.
- Keep `desktop` focused on Slint UI, dependency wiring, and process behavior.
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

- Keep the workspace dependency-light.
- Prefer `thiserror` for library errors and `anyhow` for binary/xtask boundary
  errors.
- Prefer `tracing` for observability once runtime diagnostics are needed.
- Prefer `tokio` for async Rust and `axum` for new HTTP services when a project
  actually needs a web framework.
- Required project automation belongs in Rust under `crates/xtask`.
