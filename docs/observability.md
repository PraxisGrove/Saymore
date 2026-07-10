# Observability

Use structured observability once a project needs runtime diagnostics. Do not
start with print-heavy production code.

## Recommended Stack

- `tracing` for spans and events.
- `tracing-subscriber` for formatting and filtering.
- `thiserror`/`anyhow` for errors with context.

## Guidelines

- Instrument boundaries and long-running operations.
- Prefer structured fields over formatted strings for machine-readable data.
- Avoid logging secrets, tokens, credentials, or full user-provided payloads.
- Add context where an error crosses a boundary.
- Keep domain logic independent from logging frameworks unless observability is
  part of the domain contract.

## Async Work

For async workflows, prefer instrumenting the function or boundary that owns the
operation. Avoid scattering ad hoc logs at every callsite.
