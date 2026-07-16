---
name: rust-project-dev
description: |
  Rust project engineering skill: workspace structure, multi-crate layering,
  Cargo quality gates, Rust xtask size gates, performance discipline, and intentional
  refactoring for maintainable Rust projects.
---

# Rust Project Development Skill

## When To Use This Skill

Use this skill when you need to:

- Create or refactor a Rust workspace.
- Split a project into focused crates with clear dependency direction.
- Establish mandatory quality gates.
- Add file/function size gates.
- Improve performance with measurement instead of guesswork.
- Refactor code without preserving unnecessary compatibility layers.
- Align tests with the `rust-testing` skill.

## Goals

- Keep the workspace easy for humans and AI coding agents to understand.
- Use standard Cargo commands as the required development path.
- Keep crate boundaries explicit and dependency direction one-way.
- Keep public APIs small and intentional.
- Prefer fail-fast validation with explicit errors over production assertions or
  panics.
- Run focused checks during implementation and the full workspace gate before
  pushing.
- Prefer small, reviewable changes over large ambiguous diffs.

## Verification Cadence

During implementation, run the smallest relevant checks for the changed crate
or behavior. Do not run every workspace gate merely because a task is ending.

Immediately before `git push`, run these from the workspace root:

```bash
cargo fmt --all --check
cargo check --workspace --all-targets
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace --all-targets --release
cargo run -p xtask -- size
```

Use `cargo fmt --all` to apply formatting.

## Workspace Architecture

Saymore workspace layout:

```text
crates/
  app/
  infra/
  xtask/
apps/
  desktop/
```

Responsibilities:

- `app`: business types, invariants, pure rules, use cases, and port traits.
- `infra`: concrete implementations of app ports.
- `desktop`: Slint UI, dependency wiring, and process behavior.
- `xtask`: repository maintenance and packaging automation.

Dependency direction:

```text
desktop -> app
desktop -> infra -> app
```

Keep `app` free of UI and concrete infrastructure concerns. Add another crate
only when a substantial independent interface justifies it.

## Dependency Management

- Put shared versions in `[workspace.dependencies]`.
- Member crates should use `workspace = true` where possible.
- Keep features at the boundary where they are needed.
- Use `cargo tree -d` when duplicate dependencies or feature drift appear.
- Prefer `thiserror` for library errors and `anyhow` for binary or xtask
  boundaries.
- Prefer `tracing` when runtime observability is needed.
- Prefer `tokio` for async work and `axum` for new HTTP services when a web
  framework is actually required.

## Fail-Fast Policy

- Production code must not use `unwrap`, `expect`, `panic`, `todo`, or
  `unimplemented`.
- Production assertions should not be used as input validation.
- Invalid state should be rejected at parsing, construction, configuration, or
  startup boundaries.
- Tests may use assertions to verify behavior.

## Refactoring Policy

Prefer clear replacement over long-lived compatibility layers when the project
is still a template or not constrained by production compatibility.

Refactor in coherent stages:

1. Define the target boundary.
2. Move logic into the right crate or module.
3. Add focused tests around behavior.
4. Delete the old path once migration is complete.
5. Run focused checks; reserve the full workspace gate for pre-push verification.

## Size Gates

- Target Rust files under 500 lines, excluding tests.
- Warn over 600 file lines.
- Fail over 800 file lines.
- Warn over 80 function body lines.
- Fail over 150 function body lines.

The Rust xtask size gate is approximate. It exists to catch large AI-generated
changes early, not to replace review.

## Performance Policy

Do not optimize by guesswork.

1. Measure release builds, not debug builds.
2. Profile to find hot paths.
3. Prefer algorithm and data-structure improvements before micro-optimizations.
4. Keep performance changes small and benchmarkable.

Useful release profile defaults:

```toml
[profile.release]
lto = "thin"
debug = "line-tables-only"
codegen-units = 4
strip = false
```

## References

See `references/README.md` for more detailed guidance.
