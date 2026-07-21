<p align="center">
  <img src="apps/desktop/icons/saymore-mark-3d-136.png" width="96" alt="Saymore logo">
</p>

<h1 align="center">Saymore</h1>

<p align="center">
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
  · <a href="docs/README.md">Documentation</a>
  · <a href="CONTRIBUTING.md">Contributing</a>
</p>

Saymore turns speech into text at the current cursor without making you switch
to a separate editor. Start dictation with a global shortcut, speak naturally,
and let Saymore recognize, optionally refine, and insert the final text into the
app you are already using.

Saymore is built in Rust with [Slint](https://slint.dev/) and keeps its core
speech, refinement, and storage choices explicit rather than tying the product
to one provider.

## Why Saymore

- **Works where you type.** Use one dictation flow across editors, browsers,
  chat apps, terminals, and other desktop text fields.
- **Local-first by design.** Use local recognition when you want speech to stay
  on your machine, with encrypted local history and configurable retention.
- **Provider choice.** Select local or cloud ASR and configure optional LLM
  refinement without coupling the application to a single vendor.
- **Faithful text refinement.** Clean up filler, punctuation, and structure
  without turning dictation into a chatbot or inventing content.
- **Results do not silently disappear.** Failed delivery keeps recoverable text
  available instead of dropping what you said.

## How It Works

```text
Global trigger
    -> record speech
    -> recognize locally or with your configured ASR provider
    -> apply safe local cleanup
    -> optionally refine with your configured LLM provider
    -> normalize confirmed spellings
    -> insert the final text at the current cursor
```

Local recognition can keep audio processing on-device. Choosing a cloud ASR
sends audio to that provider; enabling cloud refinement sends the transcript to
the configured LLM provider. Saymore does not read screen context, generate
replies, or automatically send messages. See the
[product direction](docs/product/open-source-voice-input-wayfinder.md) for the
full data and feature boundaries.

## Download and Status

Saymore is under active development. The macOS and Windows applications now
share the main dictation workflow and most user-facing features, with
platform-specific integration implemented natively on each system.

| Platform  | Distribution                                                                                                                         |
| --------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| Windows   | Installers are distributed through [GitHub Releases](https://github.com/PraxisGrove/Saymore/releases/latest).                        |
| macOS 12+ | Direct downloads are distributed through GitHub Releases. A Mac App Store release is planned after its submission workflow is ready. |

Direct releases include checksums for verifying downloaded artifacts.

## Development

The production desktop application uses Rust and Slint. Node.js and a web
frontend are not part of the build.

On macOS, start the persistent development preview:

```bash
./scripts/dev-preview.sh
```

On Windows, build the desktop application with Cargo:

```powershell
cargo build -p saymore-desktop
```

Read the [development guide](docs/development.md) for prerequisites, preview
behavior, packaging, and the complete quality gate. The workspace follows this
dependency direction:

```text
desktop -> app
desktop -> infra -> app
```

See [Architecture](docs/architecture.md) for crate ownership and platform
boundaries.

## Documentation

- [Product direction and scope](docs/product/open-source-voice-input-wayfinder.md)
- [Architecture](docs/architecture.md)
- [Development](docs/development.md)
- [Testing](docs/testing.md)
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
