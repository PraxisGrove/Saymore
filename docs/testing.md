# Testing

Tests should make the intended behavior easy for humans and coding agents to
understand. Prefer stable contracts over implementation details.

## Principles

- Test behavior, not private structure.
- Prefer comparing complete values over asserting field by field.
- Do not add tests for static constants.
- Do not add negative tests for logic that was removed.
- Do not expose production APIs only to make tests easier.
- Keep fixtures small and explicit.

## Layout

Use unit tests for pure logic and edge cases close to the owning module.

Use integration tests in the owning crate's `tests/` directory for public
behavior and binary workflows. For example, CLI smoke tests live under
`crates/cli/tests/`.

When test helpers become shared across crates, move them into a dedicated
test-support crate instead of duplicating setup or exposing production internals.

## Required Test Commands

Run normal Rust tests with nextest:

```bash
cargo nextest run --workspace --all-targets
```

Run doctests with Cargo because nextest does not execute doctests:

```bash
cargo test --workspace --doc
```

## AI-Assisted Changes

When an AI agent changes behavior, require tests that cover the externally
visible result. If a change is mostly refactoring, keep tests focused on the
existing behavior that should remain stable.

Generated or snapshot-like outputs should be reviewed as artifacts, not accepted
blindly. If snapshots are introduced later, document how to inspect and accept
them before adding the tool as a required dependency.
