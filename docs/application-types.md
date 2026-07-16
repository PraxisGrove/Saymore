# Application Types

This repository is a single Rust workspace. Saymore has committed to the
desktop-only direction with Slint; the other directions remain reference
guidance rather than active product stacks.

Only dependencies required by Saymore's selected direction belong in the
workspace.

## Supported Directions

| Type | Use when | Default stack |
|---|---|---|
| Server plus full-stack frontend | The product needs an HTTP API and browser UI written primarily in Rust. | `tokio`, `axum`, `leptos`, `tracing`, OpenTelemetry, `sqlx`, PostgreSQL. |
| Server plus desktop client | The product needs a backend and an installed desktop app. | Server stack plus `slint`; share contracts through a dedicated crate. |
| Desktop-only app | The product is local-first or does not need a hosted backend. | `slint`, Rust application wiring, local `infra` adapters, and SQLite with `rusqlite` when needed. |
| Server-only service | The product is an API, worker, or backend service. | `tokio`, `axum`, `tower`, `tracing`, OpenTelemetry, `sqlx`, PostgreSQL. |
| CLI/TUI app | The product is a command-line tool, developer tool, or terminal UI. | `clap`, `anyhow`, `thiserror`, `tracing`, optional `ratatui` and `crossterm`. |

## Selection Rules

- Pick server-only when the product has no user-facing installed or browser UI.
- Pick server plus full-stack frontend when Rust should own both the backend and
  the web UI.
- Pick desktop-only when the app can run locally without a hosted backend.
- Pick server plus desktop client when the desktop app depends on hosted state,
  sync, billing, accounts, or shared team data.
- Pick CLI/TUI when the main interface is a shell command or interactive
  terminal workflow.

## Policy

- Keep the workspace dependency-light.
- Do not add framework dependencies for every possible application type.
- Add dependencies when the chosen application type actually needs them.
- Keep the current `app`, `infra`, `desktop`, and `xtask` responsibilities clear.
  Split another crate only when a concrete product boundary justifies it.
- Document application-specific choices in `docs/technology-stack.md` and
  `docs/architecture.md` when the project commits to them.
