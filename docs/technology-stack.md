# Technology Stack

The template targets mature Rust projects. Add runtime dependencies only when
the project needs them, but keep the verification toolchain strong by default.

## Default Policy

- Required path: Rust, Cargo, `cargo-nextest`, `cargo-deny`, and crates in this
  workspace.
- Optional convenience tools: `just`, `prek`, and release helpers.
- Project tasks belong in `crates/xtask`, not ad hoc scripts in another
  language.

## Recommended Crates

| Area | Recommendation |
|---|---|
| CLI | `clap` |
| Serialization | `serde`, `serde_json`, `toml` |
| Library errors | `thiserror` |
| Binary/xtask errors | `anyhow` |
| Observability | `tracing`, `tracing-subscriber` |
| Async runtime | `tokio` |
| HTTP client | `reqwest` |
| HTTP server | `axum` |
| gRPC | `tonic` |
| SQL | `sqlx` |
| ORM | Prefer no ORM; use `sea-orm` only when the project needs an entity model. |
| Parameterized tests | `rstest` |
| Property tests | `proptest` |
| Snapshot tests | `insta` |
| Trait mocks | `mockall` |
| HTTP mocks | `wiremock` |
| Benchmarks | `criterion` |
| TUI | `ratatui`, `crossterm` |

## Default Application Stacks

Use these defaults when evolving this template into a concrete product. Do not
add all of them to the base workspace.

| Application type | Default stack |
|---|---|
| Server-only service | `tokio`, `axum`, `tower`, `tower-http`, `serde`, `thiserror`, `anyhow`, `tracing`, `tracing-subscriber`, `opentelemetry`, `opentelemetry-otlp`, `sqlx`, PostgreSQL |
| Server plus full-stack frontend | Server-only stack plus `leptos` for the Rust full-stack web boundary |
| Desktop-only app | `tauri` as the desktop shell, Rust command handlers, local adapters in `infra`, and optional SQLite through `sqlx` |
| Server plus desktop client | Server-only stack plus a `tauri` desktop client; share API contracts through a dedicated crate |
| CLI/TUI app | `clap`, `anyhow`, `thiserror`, `tracing`, optional `ratatui` and `crossterm` for terminal UI |

## Server Stack

Choose this for APIs, workers with HTTP control planes, and backend services.

- Runtime: `tokio`.
- HTTP: `axum` with `tower`/`tower-http` middleware.
- Data: PostgreSQL with `sqlx`.
- Observability: `tracing`, `tracing-subscriber`, OpenTelemetry traces through
  OTLP.
- Errors: `thiserror` in library crates, `anyhow` at binary and `xtask`
  boundaries.
- Configuration: environment variables parsed at startup into typed config.
- Migrations: `sqlx migrate`.

Keep `domain` free of `tokio`, HTTP, SQL, filesystem, and process concerns.
Put request handlers, runtime wiring, and shutdown behavior at the binary
boundary.

## Full-Stack Web Stack

Choose this when the frontend should also be Rust.

- Web framework: `leptos`.
- Server integration: mount Leptos on the `axum` server boundary.
- Shared types: keep request/response DTOs in a dedicated crate only when they
  are used by both client and server.
- Styling: add the CSS/tooling stack only when the UI work starts.

Do not introduce a separate JavaScript full-stack framework by default. If a
project intentionally chooses React/Next.js, document that as a project-specific
override.

## Desktop Stack

Choose this for installed desktop apps.

- Desktop shell: `tauri`.
- UI: use Tauri's web frontend boundary; choose the frontend framework per
  product UI needs.
- Rust side: expose small commands that call into `app` use cases.
- Local persistence: SQLite through `sqlx` when persistence is needed.
- Packaging: keep platform signing, notarization, and installer work in
  project-specific docs once the app has a release target.

Avoid putting desktop framework types in `domain` or `app`. Keep them in the
desktop entrypoint crate.

## Server Plus Desktop Stack

Choose this when the product has both hosted services and an installed client.

- Server: use the server-only stack.
- Desktop: use the desktop stack.
- Contract: define API DTOs and client behavior explicitly; do not let the
  desktop app depend on server internals.
- Authentication and update channels are project-specific and must be
  documented when selected.

## CLI/TUI Stack

Choose this for command-line tools, developer tools, automation CLIs, and
terminal user interfaces.

- Argument parsing: `clap`.
- Errors: `anyhow` at the binary boundary, `thiserror` in reusable library
  crates.
- Output: write human-readable output by default; add `serde_json` output only
  when scripts or integrations need stable machine-readable output.
- Logging: `tracing` with an env filter for diagnostics; keep normal command
  output separate from logs.
- TUI: `ratatui` with `crossterm` when the app needs an interactive terminal
  interface.
- Testing: use `assert_cmd` and `predicates` for CLI behavior; keep TUI state
  transitions testable without a terminal.

Keep parsing and process exit behavior in `cli`. Move reusable command logic
into `app` so it can be tested without spawning the binary.

## Web Framework Guidance

Prefer `axum` for new HTTP services. It fits the Tokio/Tower ecosystem, keeps
handlers explicit, and composes well with middleware.

Use `actix-web` when the team specifically wants Actix's model or has a strong
performance/operational reason.

Use `rocket` when a project values a more batteries-included web framework and
the tradeoff is intentional.

Do not add a web framework to the base template. Add it only to a project or a
separate template variant that actually serves HTTP.
