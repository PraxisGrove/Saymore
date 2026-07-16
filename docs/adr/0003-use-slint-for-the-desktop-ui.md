# Use Slint For The Desktop UI

Status: accepted
Date: 2026-07-12

## Context

Saymore targets macOS and Windows and already keeps its application and platform
behavior in a Rust workspace. The first desktop prototype used
Tauri 2 with React, TypeScript, and Vite. That prototype validated macOS
permissions, global-shortcut capture, and text delivery, but it also introduced
a WebView boundary and a separate frontend toolchain.

The product now prioritizes one Rust-centered desktop stack, predictable idle
resource use, Cargo-only development gates, and shared UI behavior across
macOS and Windows.

## Decision

Use Slint for the production desktop UI and Rust for all application, platform,
audio, provider, storage, and release-automation code. Remove Tauri, React,
TypeScript, Vite, Node.js, HTML, CSS, and the WebView from the production app
after the Slint replacement reaches behavioral parity.

Slint `.slint` files are permitted as compiled UI declarations. They may refer
only to UI-facing properties, callbacks, and view models. The application crate
remains framework-independent.

The same Slint component system serves macOS and Windows. OS capabilities such
as Accessibility/UI Automation, global shortcuts, credentials, login startup,
and text delivery remain separate Rust adapters.

## Consequences

- Cargo becomes the only required application build toolchain.
- The desktop app no longer needs a JavaScript package manager or WebView IPC.
- The Tauri/React prototype was migration input and has been removed; there is
  no second supported frontend.
- Slint's smaller ecosystem and custom-rendered controls increase the need for
  component-level design discipline and visual testing on both platforms.
- Lower memory use is an expected benefit, not the sole acceptance criterion;
  startup time, idle CPU, accessibility, text rendering, and UI quality must be
  measured in release builds.
- Packaging, signing, and notarization are no longer delegated to Tauri CLI and
  must be owned by `xtask` plus platform tools.
- Saymore uses the Slint Royalty-free 2.0 license and preserves its required
  public "Made with Slint" attribution; it does not adopt GPL-3.0 for Saymore.

## Alternatives Considered

- Keep Tauri and React: fastest continuation from the prototype, but retains
  the WebView and dual Rust/TypeScript toolchains.
- `egui`: pure Rust immediate-mode UI, but less suitable for Saymore's planned
  settings, onboarding, history, and form-heavy provider configuration.
- `iced`: Rust-native and capable, but Slint's declarative component model and
  designer-oriented layout are a better fit for the planned desktop product.
- Separate SwiftUI and WinUI clients: strongest platform-native appearance,
  but duplicates UI implementation and adds Swift/C# stacks.

## Migration Acceptance

- The current permission, shortcut, and delivery prototype works through Slint.
- No production build requires Node.js, pnpm, React, TypeScript, Vite, Tauri, or
  a WebView.
- macOS produces a launchable `.app` and DMG through Cargo/xtask-driven steps.
- The required workspace Cargo gates pass.
