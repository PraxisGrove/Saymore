<p align="center">
  <img src="apps/desktop/icons/saymore-mark-3d-136.png" width="96" alt="Saymore logo">
</p>

<h1 align="center">Saymore</h1>

<p align="center">
  <strong>Speak naturally. Type anywhere.</strong><br>
  Local-first voice typing for macOS and Windows.
</p>

<p align="center">
  <a href="README.md">English</a> | <a href="README.zh-CN.md">简体中文</a>
</p>

<p align="center">
  <a href="https://github.com/PraxisGrove/Saymore/actions/workflows/ci.yaml"><img src="https://github.com/PraxisGrove/Saymore/actions/workflows/ci.yaml/badge.svg" alt="CI status"></a>
  <a href="https://github.com/PraxisGrove/Saymore/releases/latest"><img src="https://img.shields.io/github/v/release/PraxisGrove/Saymore?display_name=tag" alt="Latest release"></a>
  <a href="https://github.com/PraxisGrove/Saymore/releases/latest"><img src="https://img.shields.io/badge/platform-macOS%20%7C%20Windows-4b5563" alt="Supported platforms: macOS and Windows"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-PolyForm%20Shield%201.0.0-d97706" alt="PolyForm Shield 1.0.0 license"></a>
</p>

<p align="center">
  <a href="https://github.com/PraxisGrove/Saymore/releases/latest"><strong>Download</strong></a>
  · <a href="#what-you-can-do">Features</a>
  · <a href="docs/README.md">Documentation</a>
  · <a href="CONTRIBUTING.md">Contributing</a>
</p>

Saymore turns speech into text at the current cursor, so you can dictate in the
editor, browser, chat app, terminal, or other text field you already use. Press
a global shortcut, speak naturally, and Saymore recognizes, optionally refines,
and inserts the result without opening a separate writing surface.

It is a native Rust and [Slint](https://slint.dev/) desktop application with
explicit boundaries between speech recognition, text refinement, storage, and
platform integration. You choose the providers; Saymore does not require a
hosted Saymore account or backend.

> **Project status:** Saymore is usable and distributed for macOS and Windows,
> but remains under active development. The latest release may lag behind the
> repository state.

## What You Can Do

| Area                    | Available today                                                                                                                                                                                               |
| ----------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Cross-app dictation** | Start and stop from a configurable global shortcut, then insert text into the focused editable control. Defaults are Right Command on macOS and Right Alt on Windows.                                         |
| **Speech recognition**  | Use macOS Dictation, Volcengine, or a custom OpenAI-compatible speech endpoint. Availability depends on the operating system and provider configuration.                                                      |
| **Optional refinement** | Conservatively clean up filler, punctuation, self-corrections, and structure with SenseNova or DeepSeek. Invalid or unavailable refinement falls back to the recognized text.                                 |
| **Personal dictionary** | Add, edit, delete, search, filter, and import canonical spellings from CSV. Saymore can also learn terms from repeated corrections in supported text controls.                                                |
| **Private history**     | Search, inspect, copy, delete, or clear encrypted local history, with configurable retention. Original audio is not stored.                                                                                   |
| **Desktop controls**    | Onboarding, permission checks, microphone selection, multiple shortcuts, launch at login, system tray controls, themes, English and Simplified Chinese UI, update checks, and privacy-safe diagnostic export. |

When text insertion cannot be verified, Saymore keeps the final transcript
available for recovery instead of silently discarding it.

## How It Works

```text
Global shortcut
    -> record speech in memory
    -> recognize with the configured ASR provider
    -> apply deterministic local cleanup
    -> optionally refine with the configured LLM provider
    -> normalize confirmed dictionary spellings
    -> insert at the current cursor
```

The refinement stage is deliberately narrow: it improves a transcript without
answering questions, inventing facts, or turning dictation into a chatbot. If a
provider fails or its output violates Saymore's safeguards, the pipeline falls
back to the last safe text.

## Privacy Model

- Audio is held in memory for recognition and is not written to local history.
- Cloud ASR sends audio to the provider you configure. Cloud refinement sends
  the transcript and only relevant confirmed dictionary terms to the selected
  LLM provider.
- Local history is encrypted. Its encryption key is kept in the platform
  credential store, and retention can be disabled or changed by the user.
- Sensitive controls are treated specially and are excluded from history and
  correction learning.
- Diagnostics stay local and record allowlisted event identifiers, not
  transcripts, API keys, device names, paths, or raw error details.
- Saymore does not read screen context, compose replies, or automatically send
  messages.

The [product direction](docs/product/open-source-voice-input-wayfinder.md)
defines the complete data, provider, and feature boundaries.

## Download

| Platform      | Package                                                                                                                                                            |
| ------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| **macOS 12+** | A signed and notarized universal DMG for Apple Silicon and Intel Macs is available from [GitHub Releases](https://github.com/PraxisGrove/Saymore/releases/latest). |
| **Windows**   | A current-user NSIS installer is available from [GitHub Releases](https://github.com/PraxisGrove/Saymore/releases/latest).                                         |

Each direct release includes `SHA256SUMS` for artifact verification. A Mac App
Store release is planned, but is not available yet.

After installation:

1. Complete onboarding and grant microphone permission. macOS also requires
   Accessibility permission for global shortcuts and cross-app text insertion.
2. Choose and test a speech-recognition provider on the Models page.
3. Focus any editable field and use the configured shortcut to start and stop
   dictation. Press Escape to cancel an active recording.

## Development

The production application uses Rust and Slint. Node.js, a WebView, and a web
frontend are not part of the build.

On macOS, start the persistent signed development preview:

```bash
./scripts/dev-preview.sh
```

On Windows, build the desktop application with Cargo:

```powershell
cargo build -p saymore-desktop
```

The workspace has four ownership boundaries:

| Path           | Responsibility                                              |
| -------------- | ----------------------------------------------------------- |
| `crates/app`   | Business types, invariants, use cases, and port traits      |
| `crates/infra` | Filesystem, database, network, audio, and platform adapters |
| `apps/desktop` | Slint UI, dependency wiring, and process lifecycle          |
| `crates/xtask` | Repository maintenance and packaging automation             |

```text
desktop -> app
desktop -> infra -> app
```

See the [development guide](docs/development.md) for prerequisites, preview and
packaging workflows, and the complete quality gate. See
[Architecture](docs/architecture.md) for crate ownership and platform
boundaries.

## Documentation

- [Product direction and scope](docs/product/open-source-voice-input-wayfinder.md)
- [Architecture](docs/architecture.md)
- [Development](docs/development.md)
- [Testing](docs/testing.md) and [review](docs/review.md)
- [Releasing](docs/releasing.md)
- [Technology stack](docs/technology-stack.md)

The [documentation index](docs/README.md) links to the complete set of product,
engineering, ADR, and research documents.

## Contributing

Issues, design discussions, documentation feedback, and reproducible bug reports
are welcome. Please read [CONTRIBUTING.md](CONTRIBUTING.md) before starting
implementation work. The external Contributor License Agreement workflow is not
yet available, so code contributions currently require prior coordination with
the maintainers.

## License

Saymore is **source-available**, not OSI-approved open source. It is licensed
under the [PolyForm Shield License 1.0.0](LICENSE). Personal, internal
organizational, and other noncompeting uses are permitted. Providing a product
or service that competes with Saymore requires a separate commercial license
from the maintainers. Third-party assets retain their own licenses.
