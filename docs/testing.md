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

The `models_navigation` and `model_discovery_runtime` Slint integration tests
use custom main-thread executables and are excluded from Nextest discovery. The
discovery test exercises the production callback wiring against a local HTTP
service, including fetching, manual entry, failure preservation, caching, and
invalidation. Run them explicitly with:

```bash
cargo test -p saymore-desktop --test models_navigation --test model_discovery_runtime
```

CI runs these GUI tests on macOS, where a windowing environment is available.

## Live Model Directory Acceptance

On macOS, an explicitly ignored test can validate the current user's saved
DeepSeek and custom-compatible providers through the production discovery
adapter. It only sends `GET /models`; it does not change provider settings or
send transcripts. Output contains counts and error categories, never keys or
response bodies. Run it only when real-service acceptance is requested:

```bash
cargo test -p template-infra --test model_discovery_live -- --ignored --nocapture
```

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
- model-page text clarity and high-DPI movement across displays using the
  [visual acceptance procedure](#windows-model-page-visual-acceptance), Chinese
  user paths, and long model paths;
- uninstall and portable removal with application data intentionally retained;
- Authenticode, SmartScreen reputation, and the active antivirus product.

Automation on a development machine is evidence for the deterministic portion,
not a substitute for the clean-machine, reputation, microphone, or third-party
application checks.

### Windows model-page visual acceptance

Run the packaged release EXE with the production FemtoVG renderer and normal
Windows font fallback. Remove test overrides such as `SLINT_SCALE_FACTOR` and
Windows compatibility-mode DPI overrides. Record the app version, Windows build,
display resolution, display scaling, Windows text-size setting, and GPU/driver.
Use the display's native resolution and the default text-size setting for the
baseline; record any additional accessibility text-size checks separately.

Cover 100%, 125%, and 150% display scaling on a low-density display, plus 200%
on a high-density display where available. Mark unavailable combinations as not
tested. For each combination, check Simplified Chinese and English in light and
dark appearance at the default 920 × 700 logical window size and the minimum 820
× 560 size supported by the Models page:

- Open both Transcription and Polishing. Check provider names, source labels,
  summaries, status badges, and the polishing enablement label and helper.
- Open Qwen3-ASR, Whisper, SenseVoiceSmall, and Paraformer details. Check
  metadata, download size, runtime memory, device status, download actions, and
  usage tips. Cover not-downloaded, downloading, paused, installed,
  activation-pending, and failure states; include a three-digit download
  percentage and a long error.
- On Paraformer, check both punctuation modes, missing and installed punctuation
  models, download progress, and an error. Check recognition-test idle, running,
  success, and failure states. Use a disposable test profile for downloads and
  failure setup; do not corrupt or delete a user's installed models.
- Verify that enlarged text has no clipped strokes or overlapping controls.
  Standard status and action labels must fit. Long descriptions and errors must
  wrap, and the dialog must scroll to the final usage tip. Provider names and
  summaries may elide at narrow widths, but must not collide with badges or
  icons.
- Move the window between displays with different scaling in both directions,
  with a local-model dialog open and closed. Compare with a fresh launch on each
  display: text must redraw at the destination scale without persistent blur,
  incorrect geometry, or a restart requirement.

Capture lossless PNG screenshots at original pixel dimensions after animations
settle. Inspect them at 1:1 alongside the live app; do not judge clarity from a
resized chat preview or a recompressed JPEG. Background dimming under a modal is
expected and is not a blur failure. Capture the unobscured page and the dialog
separately. Record pass/fail/not-tested for every scale, locale, appearance, and
cross-display case, attach screenshots, and keep Windows visual acceptance
pending until this matrix has actually run. macOS GUI tests validate shared
navigation and compilation, not Windows rasterization or DPI behavior.
