# Test Layout

Choose layout based on the behavior being tested.

## Integration Tests

Use `crates/<crate>/tests/*.rs` for public behavior and binary workflows.

```text
crates/cli/
  src/main.rs
  tests/smoke.rs
```

Integration tests should use the crate as an external caller would.

## Unit Tests

Use module-local tests for pure logic and private invariants when keeping the
test close to the code improves readability.

```rust
#[cfg(test)]
mod tests {
    use super::*;
}
```

## Shared Helpers

When helpers are shared across crates, prefer a dedicated test-support crate or
an explicit test module. Do not add production public APIs only for tests.
