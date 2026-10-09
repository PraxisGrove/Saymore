# Model Lists

On the Models page, open a provider card, enter its API key and service URL, and
click **Get models**. The request runs in the background and the control shows
its loading state until a list or a readable error is available. Fetching a
directory does not test, enable, or switch the provider.

## Speech Recognition

Custom OpenAI-compatible speech services fetch `GET /models` relative to the
configured API base URL. A full `/audio/transcriptions` URL is also accepted;
model discovery replaces that suffix with `/models`. Results are normalized,
sorted, and deduplicated. The current model stays selected when present in the
response; otherwise the first returned model is selected. The dropdown and
editable model ID remain available together after fetching.

Volcengine streaming recognition uses its two application-supported resource
IDs. Local models use the pinned download manifests. These are preset lists, not
remote account discovery.

## Text Polishing

SenseNova, DeepSeek, OpenAI, Kimi, Gemini, OpenRouter, MiniMax, SiliconFlow, and
StepFun use their configured model-list endpoints. Custom compatible services
use `/models` relative to their base URL; a full `/chat/completions` URL is also
accepted. A loopback custom service can fetch without an API key. Remote
services require a key. A provider's model list does not guarantee that every
returned model supports chat completions or speech recognition; the connection
test remains the check before activating a new configuration.

Qwen, Volcengine Ark, and Zhipu GLM currently show **Preset models** because
Saymore has not adapted an account model-list endpoint for those profiles. Their
preset-list status explicitly identifies this limitation. Every polishing
provider also permits manual entry of a model ID, including after a failed
fetch.

Successful polishing directories are cached with the provider, endpoint, and
profile. A failed request or cache write retains the previous list and model.
Selecting from a cached list with unchanged saved credentials preserves the
existing immediate-selection behavior. Choices made with unsaved credentials or
from a preset list remain drafts until **Test and use** succeeds. Canceling and
reopening restores the saved key/model pair; a directory fetched with draft
credentials cannot replace the confirmed model. The confirmed model remains
visible even when it is absent from the newly cached directory.

Changing the provider, tab, credentials, or service URL, or closing its dialog,
invalidates any outstanding refresh and clears its loading state. Responses from
invalidated requests cannot update the displayed list or cached directory. Only
one refresh for the current configuration is admitted at a time.

## Acceptance Progress — 2026-10-08

Objective: accept the model-directory workflow through real adapters and native
UI interactions, including cancellation and stale responses.

Completed evidence:

- The production discovery adapter fetched 2 DeepSeek models and 28 models from
  the saved custom-compatible service. These were read-only directory requests.
- Native UI interaction verified custom ASR fetching, dropdown selection, and
  manual entry. It exposed a stale dropdown label after manual entry following a
  dropdown selection.
- Review found that cancel/reopen could combine a saved key with a cached draft
  model. Restoration now prefers the confirmed model and keeps it visible; the
  runtime regression test passed after this fix.

Acceptance changes touch `settings_ui/model_discovery/catalog.rs`, the runtime
and live discovery tests, the test support module,
`asr-provider-components.slint`, `components.slint`, and the model/testing
documentation. The ASR selector now uses a two-way model binding, and its shared
chevron uses the existing SVG icon.

## Retest — 2026-10-09

The latest binding and SVG-chevron fixes passed native UI retesting using an
isolated fixture with the production Slint components and discovery callbacks.

- Custom ASR: fetched the directory, selected another model, then manually
  entered a model ID. The dropdown and input both showed the new value, and the
  SVG chevron rendered correctly.
- Polishing: fetched and selected models, changed to a temporary key, fetched
  its different directory, canceled, and reopened. The saved model was restored;
  the saved key remained unchanged in the fixture catalog.
- Stale response: started a six-second delayed request, changed credentials and
  finished a new refresh within 2.9 seconds. After the old request had finished,
  the UI still showed the new six-model directory. The persisted directory also
  retained six entries and contained no stale model.
- Qwen, Ark, and GLM: native controls and feedback explicitly showed preset
  models. Remote account discovery for these providers remains unimplemented.

Verification passed with Rust 1.95: the two desktop runtime executables, six
adapter tests, the explicit live adapter test, Clippy for all affected crates
and targets, formatting, diff whitespace, semantic colors, and size checks. The
size gate reported only the seven existing warnings and no errors. The live
adapter fetched 2 DeepSeek models and 28 custom-compatible models again using
read-only directory requests.

Local screenshot evidence lives under `target/model-acceptance/` (excluded from
Git): `speech-synced.png`, `qwen-presets.png`, and `polishing-latest.png`. The
visual fixture is not registered in Cargo and its process is stopped after
retesting. The production application bundle was not repackaged or replaced.

## Push Verification — 2026-10-09

The full pre-push gate initially passed compilation, Nextest, doc tests, Clippy,
release builds, and size checks, but dependency auditing blocked the push.
Compatible patches update `h2` to 0.4.16, `rustls` to 0.23.45, `webbrowser` to
1.2.2, and the withdrawn `chacha20` version to 0.10.2. Rustls also updates its
required `rustls-webpki` patch to 0.103.15. Cargo regenerated the lockfile; no
model-provider behavior changed in this fix.

After the dependency update, every required pre-push gate passed. Both
main-thread model UI tests passed again, and the read-only live adapter test
again returned 2 DeepSeek models and 28 custom-compatible models. The final diff
was reviewed against repository standards and the originating model-list
request. The preset-only provider limitations above remain unchanged.

## CI Toolchain Failure — 2026-10-09

[CI for c792815](https://github.com/PraxisGrove/Saymore/actions/runs/37886348488)
passed compilation checks and tests on both platforms, including the macOS model
UI tests, but failed Clippy on both platforms. Rust 1.99.0 reported
`double_must_use` on the boxed future generated by `async-trait` 0.1.89 for
`LlmProvider::refine`. Local acceptance used Rust 1.95 and did not cover that CI
toolchain.

Rust 1.99.0 reproduces the same error locally before the dependency update.
[Async-trait 0.1.92](https://github.com/dtolnay/async-trait/releases/tag/0.1.92)
fixes the generated code. Its required Syn 3 dependency is added to the
lockfile. With the update, the application crate's Clippy check and all 129
tests pass, and the dependency audit passes. Project and CI toolchains now pin
Rust 1.99.0 together; the Clippy warnings-as-errors gate remains enabled.

The full workspace check additionally exposes `map_or_identity` in elapsed-time
and shortcut-index conversions. Their equivalent `unwrap_or` forms retain the
existing overflow fallbacks on both platforms. Constant-size PCM decoding uses
`as_chunks::<2>` to satisfy `chunks_exact_to_as_chunks`, preserving
little-endian decoding and the handling of incomplete trailing bytes.

The complete local `cargo clippy --workspace --all-targets -- -D warnings` check
passes with Rust 1.99.0 after these compatibility changes.
