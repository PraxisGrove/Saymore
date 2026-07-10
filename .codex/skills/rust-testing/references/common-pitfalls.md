# Common Testing Pitfalls

- Testing implementation details instead of behavior.
- Exposing production APIs only for tests.
- Mocking the logic under test instead of I/O boundaries.
- Relying on real time, real network, or global shared state.
- Using sleeps for synchronization.
- Over-specifying mocks so refactors break tests without behavior changes.
- Adding tests for static constants.
- Adding negative tests for logic that was removed.
- Building large fixtures where small explicit values would be clearer.
- Ignoring warnings from clippy or formatting gates.
