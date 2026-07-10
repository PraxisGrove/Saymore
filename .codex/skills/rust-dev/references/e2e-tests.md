# E2E Tests

The base template is Rust-only and does not prescribe Python, JavaScript, or any
other e2e framework.

Add e2e tooling only when the project has a concrete workflow to verify. Keep it
optional and document:

- How to build the binary under test.
- How to run the e2e suite.
- Which external services are mocked or required.
- Which tests are smoke, regression, slow, or quarantined.

Prefer Rust-based integration tests first. Introduce non-Rust tooling only when
it provides clear value for the project.
