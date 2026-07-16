# Saymore Technology Stack

Saymore is a native desktop product with a Rust-first codebase. The production
desktop application must not depend on JavaScript, TypeScript, React, Vite,
Node.js, a browser runtime, or a WebView.

Slint uses declarative `.slint` UI files in addition to `.rs` files. In this
project, "all-Rust desktop stack" means that application logic, platform
integration, networking, storage, audio, and packaging automation are Rust,
while UI structure is expressed with Slint's compiled declarative language.

The decision and migration consequences are recorded in
`adr/0003-use-slint-for-the-desktop-ui.md`.

## Committed Stack

| Area | Choice | Responsibility |
|---|---|---|
| Language | Current stable Rust (1.97.0 locally), Edition 2024 | All application and platform logic |
| Desktop UI | `slint` | Settings, onboarding, history, status overlay, and result overlay |
| Async runtime | `tokio` | Network requests, model downloads, and cancellable background work |
| UI/background bridge | Typed Rust messages | Keep the Slint event loop responsive and isolate long-running work |
| Audio capture | `cpal` | Cross-platform input-device discovery and PCM capture |
| HTTP client | `reqwest` with Rustls | Cloud ASR, LLM providers, model manifests, and updates |
| Serialization | `serde`, `serde_json`, `toml` | Configuration and provider protocols |
| Local database | SQLite through `rusqlite` | Settings, history, dictionary, and model metadata |
| Provider configuration | Versioned JSON restricted to the current user | Provider activation, model, and user-supplied API keys |
| Secrets | OS credential-store adapters behind `SecretStore` | Account/session credentials and the local history data key |
| Errors | `thiserror`; `anyhow` at binary and xtask boundaries | Explicit library errors and entrypoint context |
| Observability | `tracing`, `tracing-subscriber` | Local diagnostics without user-content logging |
| Tests | Cargo test, `cargo-nextest`, `rstest` | Unit, integration, and provider contract tests |
| Dependency policy | `cargo-deny` | License, advisory, source, and duplicate checks |
| Packaging automation | Rust in `xtask` plus platform signing tools | `.app`/DMG on macOS and installer artifacts on Windows |

Versions are pinned through `Cargo.lock` when each dependency is introduced.
Dependencies are added only when the vertical slice that uses them starts; this
table is a decision, not permission to add every crate immediately.

On macOS, provider configuration lives at
`~/Library/Application Support/Saymore/config.json`. Its directory and file
permissions are restricted to the current user (`0700` and `0600`). This is a
deliberate local-file configuration mode compatible with user-managed provider
keys; the application must never log those values. Credentials issued by a
Saymore account and the local history encryption key remain in Keychain.

The MVP has no hosted Saymore backend. If accounts, billing, or sync later make
one necessary, it must also use Rust with Tokio and Axum and communicate with
the desktop app through an explicit versioned API.

## Desktop UI Boundary

Slint owns rendering, window composition, component state, and user events. It
does not own speech recognition, persistence, permissions, shortcuts, text
delivery, or provider calls.

The desktop binary wires Slint callbacks to `app` use cases and converts
application state into small UI view models. `domain` and `app` must not depend
on Slint types. Platform adapters and framework-specific handles stay in the
desktop entrypoint or `infra`.

Slint is a custom-rendered cross-platform UI toolkit. It produces native
desktop binaries, but it does not make every control an AppKit or WinUI widget.
Platform-specific behavior must therefore be validated on both operating
systems instead of inferred from the macOS appearance.

## Platform Integration

macOS adapters use the relevant Accessibility, Core Graphics, AppKit, Keychain,
audio, and code-signing APIs. Windows adapters use the `windows` crate for UI
Automation, input, credentials, startup behavior, and installer integration.
Cross-platform wrappers are acceptable only when they preserve the behavior
Saymore needs; global shortcuts and text delivery remain explicit platform
adapters.

## Packaging

Cargo remains the source of truth for builds. `xtask` coordinates resource
generation, bundle assembly, hashes, update manifests, and release checks.
Apple and Microsoft platform tools still perform code signing, notarization,
and installer creation where the operating system requires them.

The target artifacts are:

- macOS: a signed `.app` bundle distributed inside a DMG.
- Windows: a signed application and one user-facing installer format selected
  during the Windows vertical slice.

There is no Tauri CLI or Node.js packaging step in the target stack.

## Pre-Push Quality Gate

Run focused checks during implementation. Immediately before `git push`, run the
complete repository gate:

```bash
cargo fmt --all --check
cargo check --workspace --all-targets
cargo nextest run --workspace --all-targets
cargo test --workspace --doc
cargo clippy --workspace --all-targets -- -D warnings
cargo deny check
cargo build --workspace --all-targets --release
cargo run -p xtask -- size
```

`cargo check` must compile the `.slint` sources through the desktop crate's
build script, so UI compiler errors fail the standard Cargo gate.

## Explicitly Rejected For The Product

- Tauri as the desktop shell.
- React, TypeScript, Vite, HTML, and CSS for production UI.
- Electron or another bundled browser runtime.
- A second UI implementation per operating system.
- Slint types leaking into `domain` or `app`.
- Blocking audio, network, model, or database work on the UI event loop.

## Migration Status

The desktop UI migration completed on 2026-07-12:

- `apps/desktop` is one Rust crate with compiled Slint UI.
- Accessibility and microphone permission polling, Right Command press/release
  capture, and in-memory mono 16 kHz signed 16-bit PCM conversion are wired
  through explicit `app` ports and macOS `infra` adapters.
- React, TypeScript, Vite, Node.js, pnpm, Tauri, and WebView source and build
  configuration have been removed.
- macOS `.app` creation and local signing are driven by Rust `xtask`.

The first cloud ASR vertical slice now streams 16 kHz PCM to Volcengine over a
background WebSocket session, keeps provider partial results in memory, and
delivers one normalized final transcript through the existing macOS text
delivery adapter. Windows platform work starts only after the macOS product is
stable enough for formal packaging and release; it will reuse the same app use
cases and Slint UI while adding the platform-specific adapters.
