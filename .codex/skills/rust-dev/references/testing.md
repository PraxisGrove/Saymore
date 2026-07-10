# Testing Overview

Use the `rust-testing` skill for detailed testing guidance. This file summarizes
the project-development view.

## Test Pyramid

- Unit tests: pure logic, edge cases, invariants.
- Integration tests: public API behavior and module collaboration.
- E2E tests: a small number of critical workflows when the project needs them.

## Placement

Prefer integration tests in the owning crate's `tests/` directory for public
behavior. Use module-local unit tests for pure logic when they are clearer.

Avoid exposing production APIs only for tests. Move shared helpers into a
dedicated test module or test-support crate when duplication becomes meaningful.

## Determinism

- Avoid real network in default tests.
- Avoid sleeps as synchronization.
- Prefer injected clocks or explicit timestamps.
- Keep random tests seeded or property-based with clear invariants.

## Gates

```bash
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
```
