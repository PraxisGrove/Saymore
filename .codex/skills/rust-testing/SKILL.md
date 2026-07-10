---
name: rust-testing
description: |
  Rust testing best practices: unit tests, integration tests, deterministic
  tests, mock/fake boundaries, property tests, HTTP mocks, and maintainable test
  layout for AI-assisted Rust development.
---

# Rust Testing Skill

## When To Use This Skill

Use this skill when you need to:

- Add or refactor Rust tests.
- Choose between unit, integration, and e2e tests.
- Mock trait boundaries or external I/O.
- Use property-based tests for edge-heavy logic.
- Mock HTTP clients or servers.
- Keep tests deterministic and maintainable.

## Goals

- Test behavior and contracts, not implementation details.
- Keep tests deterministic across local machines and CI.
- Isolate external dependencies with traits, fakes, or mocks.
- Keep test helpers out of production APIs.
- Run Cargo gates cleanly.

## Workflow

1. Identify the behavior contract.
2. Choose the smallest useful test level.
3. Put the test in the owning crate or module.
4. Use fakes or mocks only at I/O boundaries.
5. Run:

   ```bash
   cargo fmt --all --check
   cargo test --workspace --all-targets
   cargo clippy --workspace --all-targets -- -D warnings
   ```

## Test Levels

- Unit tests: pure logic, invariants, and edge cases.
- Integration tests: public API behavior and cross-module collaboration.
- E2E tests: a small number of critical workflows.

Prefer integration tests for externally visible behavior, especially when an AI
agent changes behavior that users or downstream crates can observe.

## Test Layout

Common options:

- `crates/<crate>/tests/*.rs` for integration tests.
- Module-local `#[cfg(test)]` tests for pure logic.
- A dedicated test-support crate when helpers are shared across crates.

Avoid exposing production APIs only for tests.

## Mocking Guidance

- Mock or fake I/O boundaries, not the logic under test.
- Assert only behavior that matters to the contract.
- Prefer explicit fixtures over large opaque setup.
- Avoid real network in default tests.

## Property Testing

Use property-based testing when input combinations or edge cases are difficult
to enumerate manually.

Keep properties focused:

- Define the invariant clearly.
- Constrain generated input to valid domains.
- Keep case counts practical for CI.
- Convert minimized failures into fixed regression tests when useful.

## Determinism

- Avoid sleeps as synchronization.
- Avoid wall-clock assertions.
- Avoid global mutable state.
- Seed randomness or inject deterministic RNGs.
- Prefer temporary directories over fixed filesystem paths.

## References

See `references/README.md` and the focused reference files in this skill.
