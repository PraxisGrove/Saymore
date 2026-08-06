# Testing

Tests should make the intended behavior easy for humans and coding agents to
understand. Prefer stable contracts over implementation details.

## Principles

- Test behavior, not private structure.
- Prefer comparing complete values over asserting field by field.
- Do not add tests for static constants.
- Do not add negative tests for logic that was removed.
- Do not expose production APIs only to make tests easier.
- Keep fixtures small and explicit.

## Layout

Use unit tests for pure logic and edge cases close to the owning module.

Use integration tests in the owning crate's `tests/` directory for public
behavior and binary workflows. Desktop integration tests live under
`apps/desktop/tests/`.

When test helpers become shared across crates, move them into a dedicated
test-support crate instead of duplicating setup or exposing production
internals.

## Required Test Commands

Run normal Rust tests with nextest:

```bash
cargo nextest run --workspace --all-targets
```

Run doctests with Cargo because nextest does not execute doctests:

```bash
cargo test --workspace --doc
```

The `models_navigation` Slint integration test uses a custom main-thread test
executable and is excluded from Nextest discovery. Run it explicitly with:

```bash
cargo test -p saymore-desktop --test models_navigation
```

CI runs this GUI test on macOS, where a windowing environment is available.

## AI-Assisted Changes

When an AI agent changes behavior, require tests that cover the externally
visible result. If a change is mostly refactoring, keep tests focused on the
existing behavior that should remain stable.

Generated or snapshot-like outputs should be reviewed as artifacts, not accepted
blindly. If snapshots are introduced later, document how to inspect and accept
them before adding the tool as a required dependency.

## Windows Release Candidate Matrix

Every Windows candidate must pass the workspace commands above plus a release
build and NSIS packaging run. CI builds both the NSIS installer and a portable
copy of the same x64 release executable and uploads them as short-lived test
artifacts.

Deterministic tests must cover model queue restoration, Range resume, network
failure, disk exhaustion, integrity failure, atomic activation, failed Provider
switching, active-model deletion protection, history and dictionary persistence,
text-delivery focus/clipboard restoration, system-audio restoration, and
shortcut registration cleanup. The four pinned model probes provide the
real-audio matrix for Chinese, mixed speech, empty input, cancellation,
consecutive sessions, and long recordings.

Before a public Windows release, run the packaged application as a standard user
on a clean x64 machine and record results for:

- first install, launch, microphone permission, and global Right Alt behavior;
- each local model's download, pause, process exit, relaunch, resume,
  activation, recognition test, deletion protection, corruption repair, and
  reinstall;
- upgrade over a prior version with settings, encrypted history, dictionary, and
  downloaded models preserved;
- Notepad, Word, and a Chromium editor, including Chinese and mixed text;
- high-DPI movement across displays, Chinese user paths, and long model paths;
- uninstall and portable removal with application data intentionally retained;
- Authenticode, SmartScreen reputation, and the active antivirus product.

Automation on a development machine is evidence for the deterministic portion,
not a substitute for the clean-machine, reputation, microphone, or third-party
application checks.
