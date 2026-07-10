# HTTP Mocking

Use HTTP mocks when code depends on external HTTP behavior and the test should
not use the real network.

## Guidelines

- Inject `base_url` or transport configuration.
- Assert method, path, query, headers, and body only when relevant.
- Keep mock responses small and explicit.
- Prefer structured assertions over raw string matching.

Useful crates:

- `wiremock`: async-friendly server with rich matchers.
- `mockito`: lightweight HTTP mocking for simpler cases.

Do not hardcode production URLs in code that needs to be tested.
