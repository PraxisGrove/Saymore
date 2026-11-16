# Qwen3-ASR Q8_0 GGUF integration

## Objective and decision

Replace Qwen's ONNX implementation with Q8_0 GGUF. The user selected Rust-owned
application code calling speech.cpp and explicitly rejected compatibility work.
The current implementation therefore has one Qwen runtime and artifact layout;
there is no ONNX fallback, artifact migration or old Qwen compatibility branch.

The earlier [Arion feasibility study](qwen3-arion-optimization.md) motivated
this change. It remains a historical engine comparison, not acceptance evidence
for the final Rust adapter. Arion application code is not incorporated.

## Artifact

- Model: Qwen3-ASR 1.7B, Q8_0 GGUF, Apache-2.0 checkpoint.
- Model ID: `qwen3-asr-1.7b-q8_0`.
- Source: `sakasegawa/Qwen3-ASR-1.7B-GGUF` on Hugging Face.
- Revision: `81e2423d24073cdeeccb5951df16accc89e075a2`.
- File: `Qwen3-ASR-1.7B-Q8_0.gguf`.
- Size: 2,176,109,216 bytes (2.18 decimal GB; the current UI formats this as
  2.03 GB using binary units).
- SHA-256: `5f219b78a1d9c3b9e97da27708b36f8a0bc1bfc1650b541c0a6dbaf87c9a62d0`.
- URL:
  <https://huggingface.co/sakasegawa/Qwen3-ASR-1.7B-GGUF/resolve/81e2423d24073cdeeccb5951df16accc89e075a2/Qwen3-ASR-1.7B-Q8_0.gguf>.

The shared installer retains resumable downloads, exact size/hash checks, atomic
activation and verification markers. The tokenizer and weights are in one GGUF.
The adapter validates the expected file before loading; full hash verification
belongs to the installer. User-owned old files are not automatically deleted.

## Runtime and build

`crates/infra/build.rs` builds a standalone `saymore-speech` executable from:

| Source                        | Revision                                   | Source archive SHA-256                                             |
| ----------------------------- | ------------------------------------------ | ------------------------------------------------------------------ |
| `nyosegawa/speech.cpp` 0.8.11 | `c4bdcd0e751bd29460cb1353e6d54499828a1d90` | `cf233c7b44eec7b1a91c299fd7ad5b67930ec46e1e913f970fd86bc2ea13cafa` |
| `nyosegawa/ggml`              | `cbdeccfd49ec9c702c175c273d36c47520cf4710` | `03f751738259e0c0a8a6a98b15934afee8e6768aa21c815f75eb4a5a2ad40b50` |

Sources come from GitHub codeload, are verified before extraction, and are
cached in Cargo's build output. CMake always builds Release native code. CPU
builds turn off build-host-specific optimization; Windows uses the static MSVC
CRT. Apple silicon enables Metal, embedded shaders and Accelerate; the adapter
explicitly requests GPU and checks that the ready envelope reports an MTL
backend. Intel macOS and Windows explicitly select CPU. No Vulkan/CUDA path is
introduced. A selected backend's failure is an error rather than silent
fallback.

The worker links its own ggml statically. Keeping it in another process avoids
symbol/global-state conflicts with Whisper's separately pinned ggml and confines
native device initialization to that child. The desktop does not need a Python
interpreter or another downloaded runtime. Source archives are build-time
inputs; recognition is local after installation.

Cargo stages the worker beside the profile's binaries. Packager metadata lists
both executables; local macOS bundles copy the worker into `Contents/MacOS` and
sign the bundle. Universal release packaging merges both desktop architectures
and both workers. Windows portable artifacts include both executables. Runtime
lookup is relative to the application (or Cargo's examples/deps directory),
never PATH or a user-configurable arbitrary command. MIT and vendor notices ship
in `packaging/resources/third-party`.

## Recognition contract

Rust implements the existing app port, model validation, session vocabulary,
background scheduling and overlap joining. A resident child receives 16 kHz
little-endian PCM16 in base64 through private JSON Lines stdin/stdout. There is
no TCP listener. Native stderr is discarded to prevent transcript, vocabulary
and path leakage. Diagnostics include only fixed event/backend labels.

Readiness checks protocol 3, runtime 0.8.11, model name/quantization/sample
rate, compute backend and prompt capability. Requests are serialized. Every
request sets the session's deduplicated canonical vocabulary as a
space-separated `prompt`, including an explicit empty prompt. The total
vocabulary remains bounded to 128 UTF-8 bytes. The old ONNX-specific 512-token
configuration, tokenizer files, CSV option and 20-second hinted-window branch
are removed.

All sessions use 30-second windows with one-second overlap, decoded in the
background during capture; stop flushes the tail. This stage does not add
one-second previews, pause detection or draft verification. It is not native
streaming Qwen inference.

Only complete terminal responses are accepted. Missing text, mismatched request
identity, malformed/oversized output, model-limit truncation, native errors and
response timeout are errors. A failed worker is killed/reaped and not reused;
reloading the model creates a new worker. Response lines are bounded to 1 MiB;
loading and each inference response have a 120-second budget. Worker stdout is
read on its own thread. Model release kills/reaps the process and joins that
reader. Cancellation retains the shared background-worker behavior: queued audio
and callbacks are discarded, while an in-flight native call can finish before
its decoder is released. This change does not add native mid-call cancellation.

The desktop's Qwen memory value now samples the child process, not the desktop's
resident-memory delta. Sampling is unavailable during an occupied decoder lock.

## Validation and progress

Completed:

- Replaced the six-file Qwen ONNX manifest and adapter with the single pinned
  GGUF and native worker; renamed `qwen3_asr_onnx_probe` to `qwen3_asr_probe`.
- Updated provider labels, download-size fallback, build/package wiring,
  architecture, development and recognition-mode documentation.
- Infra all-target compilation passed with the native worker built by Cargo.
- Forty focused tests passed: Qwen/session/protocol, background recognition,
  process memory and model-installer behavior.
- Production verified installer downloaded and activated the GGUF under
  `target/qwen-gguf-acceptance/models`. Two real adapter sessions recognized the
  bundled public Chinese fixture as `可以通过语音进行人机交互。`.
- The native macOS executable's dependency inspection lists only system
  libraries and frameworks, with no separate ggml/speech dylib required.
- All 559 normal workspace tests passed (11 skipped, including opt-in native
  tests). Workspace all-target Clippy passed with warnings denied. The size gate
  passed with zero warnings; dependency policy passed with existing duplicate
  dependency warnings and no new dependency versions.
- The opt-in native test passed against the downloaded model: consecutive
  recognition, canonical vocabulary then empty-vocabulary isolation,
  cancellation, empty audio, silence and nonzero child memory.
- The release macOS `.app` was built with both binaries and notices. Deep/strict
  code-signature verification passed and explicitly validated `saymore-speech`.
  A copy of that packaged worker plus the production Rust replay executable
  recognized the Chinese fixture after relocation into Unicode paths, using a
  relocated model path. This verifies runtime lookup and embedded resources, not
  microphone permissions or text delivery.

### Production adapter timing

After builds and other inference checks finished, five serial Release replays on
Apple M4 used the production Rust adapter and the original fixtures. Each run
loaded the model and warmed it with the first four seconds before capture-speed
replay; refinement was disabled. Stop-to-final excludes model loading, warmup,
network refinement and text insertion.

| Fixture                      | Observations | Stop-to-final |
| ---------------------------- | ------------ | ------------- |
| Short Chinese, 3.859 s       | 3; median    | 0.408 s       |
| Continuous Chinese, 39.644 s | 1            | 1.072 s       |
| Continuous English, 35.072 s | 1            | 0.667 s       |

Continuous transcripts exactly match the Metal research worker's outputs:
Chinese CER 0/178 (0%); English WER 2/92 (2.17%), with `network` split into
`net work`. These synthetic fixtures and single continuous observations do not
establish general accuracy or stable long-audio latency. Raw results are in
`production-timing.jsonl`, with diagnostics separately in `production-*.stderr`.

Implementation and local macOS acceptance are complete. Remaining release
acceptance includes Windows execution/installer validation, Intel execution,
clean minimum-OS testing, human/noisy speech and actual stop-to-insertion
timing. These are not inferred from the local build and replay.

Logs are in `target/qwen-arion-review/`: `integration-check.log`,
`integration-jobs.log`, `workspace-checks.log`, `final-hooks.log`, `bundle.log`,
`final-native-and-bundle.log`, `final-relocation.json`,
`production-timing.jsonl` and `gguf-install.log`. The download probe overlapped
compilation; its load and inference timings are not a controlled performance
comparison. The following commands exercise the actual new adapter:

```sh
cargo run --locked -p template-infra --release --example qwen3_asr_probe -- \
  target/qwen-gguf-acceptance/models apps/desktop/assets/asr-test/standard-zh.pcm
SAYMORE_QWEN_MODEL_DIR="$PWD/target/qwen-gguf-acceptance/models/qwen3-asr-1.7b-q8_0/81e2423d24073cdeeccb5951df16accc89e075a2" \
  cargo test --locked -p template-infra --test qwen3_native -- --ignored
```
