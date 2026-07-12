# Documentation

Project guidance for humans and coding agents.

- `adr/0001-asr-providers-declare-language-capabilities.md`: ASR language
  capabilities are declared per Provider rather than as one global product list.
- `adr/0002-dual-license-mit-or-apache-2.0.md`: the client and core libraries
  use the existing Rust ecosystem-friendly dual license.
- `adr/0003-use-slint-for-the-desktop-ui.md`: the production desktop app uses
  Slint and Rust instead of Tauri and a Web frontend.
- `product/open-source-voice-input-wayfinder.md`: voice recognition product
  direction, MVP scope, architecture, validation slices, and unresolved decisions.
- `research/typeless-input-behavior.md`: first-party research on Typeless desktop
  dictation timing, insertion, processing, permissions, and known unknowns.
- `research/multilingual-support-typeless-shandianshuo.md`: first-party comparison
  of Typeless and Shandianshuo multilingual behavior and model architecture.
- `research/personal-dictionary-learning-typeless-shandianshuo.md`: first-party
  comparison of automatic vocabulary claims and manual dictionary workflows.
- `architecture.md`: crate boundaries and dependency direction.
- `development.md`: required Cargo, nextest, deny, and size gates.
- `technology-stack.md`: recommended Rust crates and framework choices.
- `application-types.md`: supported project directions and when to choose them.
- `error-handling.md`: structured error strategy.
- `fail-fast.md`: early validation without production assertions.
- `dependency-policy.md`: dependency selection and review rules.
- `observability.md`: tracing and runtime diagnostics guidance.
- `testing.md`: test layout and test authoring rules.
- `review.md`: review checklist, size guidance, and API-change expectations.
