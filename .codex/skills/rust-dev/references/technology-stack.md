# Technology Stack

Keep the base template minimal. Add dependencies only when the project needs
them.

Preferred directions:

- CLI: `clap`
- Serialization: `serde`, `serde_json`, `toml`
- Library errors: `thiserror`
- Binary and xtask errors: `anyhow`
- Observability: `tracing`, `tracing-subscriber`
- Async runtime: `tokio`
- HTTP client: `reqwest`
- HTTP server: `axum`
- gRPC: `tonic`
- SQL: `sqlx`
- Parameterized tests: `rstest`
- Property tests: `proptest`
- Snapshot tests: `insta`
- Trait mocks: `mockall`
- HTTP mocks: `wiremock`
- Benchmarks: `criterion`

Prefer `axum` for new HTTP services when a web framework is needed. Do not add a
web framework to the base template.
