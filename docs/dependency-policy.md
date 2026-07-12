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
- Keep heavy dependencies away from `domain`.
- Prefer dependency boundaries at crate edges.
- Run `cargo tree -d` when duplicate versions appear.

## Required Dependency Gate

Run `cargo deny check` before handing off a change. The repository-level
`deny.toml` defines the license, advisory, duplicate-version, and wildcard
dependency policy.

Required template tasks should be implemented in Rust under `crates/xtask`.

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
into `domain` or `app`.

Rustls uses Mozilla's CA root dataset through `webpki-roots`. The dataset is
licensed under `CDLA-Permissive-2.0`, which is explicitly allowed in
`deny.toml`; this allowance applies to the certificate data dependency and does
not change Saymore's source-code license.
