# Dependency Policy

Dependencies are architecture decisions. Add them deliberately.

## Before Adding A Dependency

Document:

- What problem the dependency solves.
- Why the standard library or an existing dependency is not enough.
- Maintenance status and release activity.
- License compatibility.
- Feature flags being enabled.
- Alternatives considered.

## Workspace Rules

- Put shared versions in `[workspace.dependencies]`.
- Member crates should use `workspace = true`.
- Keep heavy infrastructure dependencies out of `app`.
- Prefer dependency boundaries at crate edges.
- Run `cargo tree -d` when duplicate versions appear.

## Required Dependency Gate

Run `cargo deny check` as part of the pre-push gate. The repository-level
`deny.toml` defines the license, advisory, duplicate-version, and wildcard
dependency policy. During implementation, run it early when dependency metadata
changes.

Required project tasks should be implemented in Rust under `crates/xtask`.

## Slint License

Saymore uses Slint under the Slint Royalty-free Desktop, Mobile, and Web
Applications License 2.0, not GPL-3.0. That license requires attribution. The
official "Made with Slint" badge in the root README must remain visible on the
public repository and binary-download page while Saymore distributes Slint as
part of the desktop application. Removing or relocating the badge requires a
license review or an in-application `AboutSlint` screen first.

## macOS Audio Capture

The desktop recording slice uses `cpal` 0.18.1 for input-device discovery and
audio capture. Rust's standard library has no audio-device API, and `cpal`
keeps the capture port usable for the planned Windows adapter. Direct CoreAudio
bindings were rejected because they would duplicate stream negotiation and
callback safety code; playback-oriented crates such as `rodio` do not solve
input capture. `cpal` is maintained by the RustAudio organization and is
licensed under Apache-2.0.

The macOS permission adapter uses `objc2-av-foundation` 0.3 with only
`AVCaptureDevice`, `AVMediaFormat`, and `block2` features. This provides typed
access to Apple's microphone authorization API; neither the standard library
nor `cpal` owns the operating-system permission prompt. A handwritten
Objective-C bridge was rejected because it would add unsafe FFI without
reducing product dependencies. The `objc2` family is dual MIT/Apache-2.0.

## macOS Desktop Integration

The infrastructure crate uses `objc2-service-management` 0.3 with only the
`SMAppService` bindings needed to register the packaged application for login.
Rust's standard library and Slint do not expose Apple's supported login-item
API. Shelling out to legacy preference tools was rejected because it bypasses
the user's Login Items controls and does not match current macOS behavior.

Dock visibility and application activation use the existing `objc2-app-kit`
dependency with the focused `NSApplication`, `NSResponder`, and
`NSRunningApplication` features. Slint's `system-tray` feature provides the
portable status-item and menu lifecycle; handwritten AppKit menu ownership was
rejected because it would duplicate Slint's event-loop integration. The
`objc2` family is actively maintained and dual MIT/Apache-2.0.

## JSON Provider Configuration

The infrastructure crate uses `serde_json` to read and atomically write the
versioned provider configuration at
`~/Library/Application Support/Saymore/config.json`. Rust's standard library
does not include a JSON parser, and manual string parsing would make schema
evolution and escaping unreliable. The file is restricted to the current user;
provider keys must never be included in logs or diagnostics. `serde_json` is
dual MIT/Apache-2.0.

## Volcengine Streaming ASR

The first cloud ASR adapter uses `tokio` and `tokio-tungstenite` for the live
WebSocket session, `rustls` with the `ring` provider for TLS, `flate2` for the
provider's gzip-framed binary protocol, and `uuid` for provider connection and
request identifiers. The standard library does not provide TLS, WebSockets, or
gzip. A blocking WebSocket was rejected because microphone audio and provider
responses must progress concurrently without blocking the audio callback or UI
thread. These dependencies are confined to `infra`; provider types do not leak
into `app`.

Rustls uses Mozilla's CA root dataset through `webpki-roots`. The dataset is
licensed under `CDLA-Permissive-2.0`, which is explicitly allowed in
`deny.toml`; this allowance applies to the certificate data dependency and does
not change Saymore's source-code license.

## Chat Completions LLM

The OpenAI-compatible Chat Completions adapter uses `reqwest` 0.13.4 for
asynchronous HTTP, JSON request bodies, bounded response streaming, redirects,
and request timeouts. The enabled features are `json`, `rustls-no-provider`, and
`stream`; default TLS, charset, HTTP/2, and system-proxy features remain off.
The existing `rustls` dependency installs the `ring` crypto provider. Rust's
standard library has no HTTPS client, and using `hyper` directly would expose
transport details without reducing the transitive network stack. A blocking
client was rejected because dropping an asynchronous request is the provider
cancellation mechanism. `reqwest` is actively maintained and dual
MIT/Apache-2.0.

The `app` port uses `async-trait` 0.1 so a configured provider can be held as a
trait object. Native `async fn` in traits is not object-safe, while exposing a
boxed-future signature would leak executor-oriented types into every adapter.
`tokio-util` 0.7 supplies `CancellationToken`; a custom atomic flag plus async
notification would duplicate cancellation and wake-up behavior. Both crates
are actively maintained and dual MIT/Apache-2.0. Only Tokio's existing runtime,
sync, time, and macro features are used, with `test-util` enabled for app tests.

Provider contract tests use `httpmock` 0.8.3 as a development-only dependency.
It verifies HTTP method, path, headers, JSON bodies, delayed responses, and
error mapping without real network services or credentials. A handwritten TCP
server was rejected because HTTP framing and concurrent test shutdown would
become unrelated test infrastructure. `httpmock` is MIT-licensed and does not
enter production binaries.

## Local Diagnostics

The desktop crate uses `tracing` 0.1 and `tracing-subscriber` 0.3 for structured,
privacy-filtered runtime diagnostics. Packaged desktop applications do not have
a reliable terminal, and a custom logger would duplicate event formatting,
filtering, subscriber installation, and writer integration. Remote services such
as Sentry were rejected for normal ASR and LLM request failures because these
diagnostics should remain on the user's device.

Both crates are actively maintained by the Tokio project and are licensed under
MIT. Their default features are enabled: `tracing` uses `std` and `attributes`;
`tracing-subscriber` uses its formatting, ANSI, registry, smallvec, tracing-log,
and thread-local support. The installed subscriber filters on Saymore's explicit
diagnostic target, disables ANSI output, and writes through the bounded local log
adapter. Neither dependency enters the `app` crate.
