# Determinism And Stability

## Goals

- Tests should behave the same in CI and on local machines.
- Failures should be reproducible and easy to diagnose.

## Randomness

- Prefer invariant-based assertions over exact random sequences.
- Seed RNGs when random behavior must be tested.
- After property-test shrinking, copy important minimized cases into fixed
  regression tests.
- When testing random selection logic, reduce the candidate set to one valid
  option whenever possible.

## Time

- Avoid assertions based on `SystemTime::now()` or similar wall-clock values.
- Prefer injected clocks or explicit timestamps.
- If exact time does not matter, assert presence and type rather than a specific
  value.

## External Dependencies

- Use mocks or fakes for DB, Redis, HTTP, filesystem, and process boundaries by
  default.
- Use real services only for explicit e2e suites with controlled setup and
  cleanup.

## Flakiness Checklist

- No sleeps for synchronization.
- No real network in default tests.
- No hidden global mutable state.
- Property-test case counts are practical.
- Temporary files use isolated temporary directories.

## Tooling Noise

IDE diagnostics can differ from real builds because of caching or proc-macro
state. Treat `cargo test` and `cargo check` as the source of truth.
