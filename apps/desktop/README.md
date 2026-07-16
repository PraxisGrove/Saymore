# Saymore Desktop

The native Saymore desktop application, implemented with Rust and Slint. It
currently supports macOS Accessibility and microphone permission flows. Press
the right Command key once to start recording and press it again to finish;
press Escape to cancel. Saymore keeps the recording in memory and converts it
to mono 16 kHz signed 16-bit PCM for speech recognition.

## Development

```bash
./scripts/dev-preview.sh
```

`Saymore Preview.app` combines that isolated Development environment with
automatic incremental rebuild and app restart. It can run alongside the
production `Saymore.app`, although both currently listen for the same global
Right Command shortcut and should not perform dictation at the same time.
It does not read or write the production SQLite database, Provider configuration,
dictionary, diagnostics, instance lock, or history encryption key.
Preview uses a persistent local signing identity so macOS keeps its microphone
and Accessibility authorization across rebuilds. The first run after migrating
from an older ad-hoc Preview build asks once to trust the local development
certificate and requires those permissions to be enabled once again.
The Cargo binary under `target/debug` is only an intermediate build artifact,
not a third app or a supported preview entry point.

Create the signed local release bundle with:

```bash
cargo run -p xtask -- bundle-macos
```

`just preview` and `just release` are optional aliases when `just` is installed.

## Local refinement experiment

Preview is a debug build, so each successful non-sensitive dictation with history
enabled stores the following fields inside the encrypted Development history
payload:

- the ASR transcript;
- the accepted LLM-refined text when refinement completed;
- the final text after local standard-spelling normalization.

Start Preview with `./scripts/dev-preview.sh`, configure either SenseNova or
DeepSeek on the Models page, enable refinement, and dictate into an external input
field. Open History and select a record to compare the available stages. Disable
refinement and repeat the same dictation to establish the ASR/local-processing
baseline. Short transcripts can intentionally skip the LLM stage; use at least 20
CJK units or 8 English words when testing Provider behavior.

Normal release builds omit the ASR and LLM intermediate fields. A release test build
can opt into the same experiment with the `history-experiments` Cargo feature.
