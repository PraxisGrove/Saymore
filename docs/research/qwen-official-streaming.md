# Qwen official streaming algorithm: native port and comparison

## Current objective

The user selected an **official-algorithm native port**, preserving Rust,
speech.cpp, Q8_0 GGUF and Metal, and retaining both streaming and non-streaming
modes. The client now exposes that choice in Qwen's details page. This record
separates the upstream contract from the implemented port and its measurements.

Inspected Qwen3-ASR revision: `7c6daf77a2421100f5fb066495372c00129d39ff`
(`qwen-asr` 0.0.6). Saymore ports the algorithm without shipping Python/vLLM.

## What the official implementation does

The
[official example](https://github.com/QwenLM/Qwen3-ASR/blob/7c6daf77a2421100f5fb066495372c00129d39ff/examples/example_qwen3_asr_vllm_streaming.py)
uses `Qwen3ASRModel.LLM`, then `init_streaming_state`, repeated
`streaming_transcribe` calls and `finish_streaming_transcribe`.

The
[implementation](https://github.com/QwenLM/Qwen3-ASR/blob/7c6daf77a2421100f5fb066495372c00129d39ff/qwen_asr/inference/qwen3_asr.py)
explicitly requires `backend == "vllm"` in all three streaming entry points. Its
default state uses two-second audio chunks, two initial unfixed chunks and five
unfixed text tokens. Callers may supply smaller PCM pushes; the state buffers
them until a complete chunk is available. The Flask demo uses different explicit
defaults (one-second chunks, four unfixed chunks), so there is no single
mandatory preview cadence for all official callers.

For each complete chunk, it appends the chunk to cumulative audio and re-feeds
**all audio received so far** to the model. After the initial unfixed chunks, it
tokenizes prior raw output, rolls back trailing tokens, and appends the retained
prefix to the assistant generation prompt. The newly generated suffix is
combined with that prefix. Finish processes any remaining shorter tail. A final
call with no remaining tail returns the existing state unchanged.

Consequently, calling this official streaming must not imply a Zipformer-style
encoder that only computes newly arrived frames or persistent acoustic-state
reuse. Earlier descriptions contrasting "official/native" with all forms of
cumulative-window processing were too broad.

speech.cpp's existing `draft` option is different: it verifies proposed tokens
against fresh greedy predictions instead of forcing the retained assistant
prefix. Its `prompt` option supplies system-turn vocabulary context, not that
assistant prefix. The port adds a distinct `prefix` option rather than using
`draft` or shortening non-streaming windows.

## Runtime and platform facts

The official
[package metadata](https://github.com/QwenLM/Qwen3-ASR/blob/7c6daf77a2421100f5fb066495372c00129d39ff/pyproject.toml)
pins `vllm==0.14.0` for the optional vLLM backend and `transformers==4.57.6`.
The official path uses a Python runtime and the matching Hugging Face
model/tokenizer assets. It is not a switch in the speech.cpp worker that
currently loads our single Q8_0 GGUF.

vLLM 0.14.0 does **not** require Linux/CUDA in every configuration. Its
[Apple installation documentation](https://github.com/vllm-project/vllm/blob/v0.14.0/docs/getting_started/installation/cpu.apple.inc.md)
describes experimental macOS Apple Silicon CPU support, source builds only, with
FP32/FP16. Its
[build configuration](https://github.com/vllm-project/vllm/blob/v0.14.0/setup.py)
selects CPU on macOS. This is platform support, not a successful local Qwen ASR
streaming test or a performance claim. The PyPI 0.14.0 release inspected here
has Linux aarch64/x86_64 wheels and a source distribution, no macOS wheel.

The Apple documentation links the community vllm-metal plugin for Metal/MLX.
Inspected plugin revision `a178b54072ca7a73a46e242d20fbe35e691f79c4` does
contain Qwen3-ASR support, so it would be incorrect to claim that the plugin has
none. However:

- Its
  [STT documentation](https://github.com/vllm-project/vllm-metal/blob/a178b54072ca7a73a46e242d20fbe35e691f79c4/docs/stt.md)
  demonstrates Qwen3-ASR 0.6B transcription. That does not establish the
  official qwen-asr 0.0.6 streaming wrapper with the 1.7B model.
- Its
  [STT loader](https://github.com/vllm-project/vllm-metal/blob/a178b54072ca7a73a46e242d20fbe35e691f79c4/vllm_metal/stt/loader.py)
  reads configuration and safetensors/NPZ weights; it does not load Saymore's
  speech.cpp GGUF through this STT path. General text-model GGUF support does
  not establish Qwen ASR GGUF compatibility.
- Its
  [ASR adapter](https://github.com/vllm-project/vllm-metal/blob/a178b54072ca7a73a46e242d20fbe35e691f79c4/vllm_metal/stt/qwen3_asr/adapter.py)
  extracts transcript tokens from the generated language envelope. Official
  streaming prefix handling and vocabulary semantics need actual parity tests.
- The plugin's documented install requires macOS 15+, Python and its own matched
  vLLM/core build. It is an additional runtime integration, not the already
  validated Rust/speech.cpp package.

No vLLM/Metal-plugin runtime or alternative model weights were installed as part
of this source review, and no real official streaming inference was executed.

## Concrete integration choices

1. **Call the official qwen-asr streaming API.** Rust would control a worker
   hosting that API, with explicit start/audio/finish/cancel and vocabulary
   snapshots. This introduces a Python/vLLM runtime and a separately validated
   model layout. A local Apple CPU feasibility test is possible; a Metal-plugin
   combination needs independent version/model/protocol validation. A remote GPU
   deployment is another choice, but must not be silently selected for a local
   model or receive audio without the user's deployment choice.
2. **Port the official algorithm to the existing native engine.** Keep Rust,
   speech.cpp, Q8_0 GGUF and Metal, while implementing cumulative audio,
   token-based rollback and assistant-prefix continuation. This requires
   extending the native prompt/decoder interface and validating its behavior
   against the pinned official implementation. Label it an official-algorithm
   port, not direct use of the official vLLM runtime.

The user explicitly chose option 2. The existing model artifact, backend and
resident-process architecture are preserved.

## Implemented port

- `crates/infra/native/qwen-streaming.patch` extends the pinned speech.cpp
  source. The build verifies the original archive, applies the patch in an
  isolated source directory and checks that continuation support was actually
  installed. The runtime handshake rejects workers without the continuation
  options.
- Native tokenization rolls back five tokens from the previous raw language/text
  envelope and removes additional trailing tokens if needed to avoid a broken
  UTF-8 boundary. The port permits an empty prefix on the final tail as well;
  upstream's finish path retains at least one prior token when available. The
  retained tokens are part of the assistant prompt, not a speculative draft.
  Each response returns both parsed text and `raw_text`.
- Rust's `qwen3_asr/streaming.rs` owns cumulative audio, chunk index and prior
  raw output per session. It follows the upstream 2-second / 2-unfixed-chunk /
  5-token defaults, including short-tail flush and no extra decode at an exact
  boundary. Native generation has a 128-new-token budget per step; hitting a
  model/token limit is an error. This budget is a port policy, not numerical
  parity with vLLM's generation settings.
- Capture queues audio without waiting for inference. Before decoding, the
  worker drains pending steps into the latest cumulative audio. This coalesces
  outdated previews without dropping samples; the first two actual decodes
  remain unfixed. It is an explicit scheduling policy added by the port. Input
  is limited to 20 minutes, bounding queued and cumulative PCM together and
  staying inside one native model window. Cancellation suppresses subsequent
  results; a native request already in flight must finish.
- `QwenRecognitionMode` is persisted in provider configuration, validates
  unknown values and defaults to non-streaming. Saving it does not change the
  active provider. Recording startup snapshots it; settings changes apply next
  time.
- The floating recording window displays the latest 100 characters of streaming
  hypotheses and rejects queued updates belonging to an old session. Partial
  text is never inserted into the target application. Existing final refinement
  and delivery remain the output path.
- Non-streaming keeps the existing 30-second windows and one-second overlap.
  Both modes use the same loaded model and explicit per-request vocabulary
  reset.

This retains cumulative re-encoding and growing prefix cost. It does not reuse
incremental acoustic features. Earlier fixed text can remain wrong after the
five-token revision window passes; streaming is not a promise of equal accuracy
on arbitrary audio or lower stop latency.

## Same-audio comparison

Measured on the same Apple M4 with the pinned 1.7B Q8_0 model and Metal, Release
Rust adapter, warmed model, capture-rate PCM input, empty dictionary and
disabled polishing. Compilation did not run during measurement. Short-phrase
values are medians of three alternating runs per mode; continuous samples are
one run per mode. These are synthetic speech fixtures, not a representative
human corpus.

| Audio             | Non-streaming first partial | Streaming first partial | Non-streaming stop-to-final | Streaming stop-to-final | Normalized errors, non-streaming → streaming |
| ----------------- | --------------------------: | ----------------------: | --------------------------: | ----------------------: | -------------------------------------------- |
| Chinese, 3.859 s  |          None before finish |                 2.320 s |                     0.457 s |                 0.451 s | CER 0/12 → 0/12                              |
| Chinese, 39.644 s |                    33.406 s |                 2.339 s |                     1.181 s |                 1.397 s | CER 0/178 → 0/178                            |
| English, 35.072 s |                    33.407 s |                 2.457 s |                     0.741 s |                 1.493 s | WER 2/92 → 0/92                              |

First-partial timing measures the adapter callback, not a rendered frame. The
non-streaming callback is internal segment progress; its floating preview is not
enabled. Stop-to-final excludes model loading, polishing and target-editor
delivery. Chinese scoring removes punctuation/spacing; English scoring compares
lowercased word tokens. The English difference is `net work` versus `network`.
These results favor streaming for early feedback, not uniformly for final
latency or general recognition quality. Non-streaming remains the default.

An additional 118.931-second Chinese stress sample repeats the continuous
fixture three times. Streaming produced its first partial at 2.366 s and
completed 4.538 s after stop, with CER 0/534. Later decoder calls reached about
four seconds; coalescing kept all audio while skipping obsolete preview steps.
This validates nearly two minutes on this device, not the full 20-minute upper
bound or CPU-only Windows performance. Native failures and generation limits
remain explicit errors.

Reproduce through the existing production-adapter probe:

```sh
cargo build --locked --release -p template-infra --example asr_latency_probe
SAYMORE_BENCH_COLD=0 SAYMORE_BENCH_QWEN_MODE=streaming target/release/examples/asr_latency_probe qwen3 /absolute/model/directory apps/desktop/assets/asr-test/standard-zh.pcm capture disabled
# Repeat with SAYMORE_BENCH_QWEN_MODE=non_streaming.
```

Raw comparison results and the driver are in
`target/qwen-streaming-port/comparison.json` and `compare.py`; the longer run is
`long-zh.json`. Source fixtures are under `target/asr-matrix/continuous`.

## Verification

- 566 workspace tests passed, including cumulative audio, tail/exact-boundary
  handling, coalescing without sample loss, size limits and cancellation.
- Three opt-in native tests passed with the pinned model: pre-stop results,
  session/vocabulary isolation, and forcing an incorrect prefix to distinguish
  continuation from speculative draft verification.
- All three desktop GUI executables passed. Qwen's test clicks both modes,
  checks persistence and provider preservation, and captures the floating
  preview. The selector and overlay snapshots were visually reviewed.
- Clippy, repository hooks, doctest command, semantic UI colors and size gates
  passed. The size gate reported no warnings.
- The Release macOS bundle was built and its nested signatures verified. Its
  worker was copied with the probe to a path containing Chinese and a space,
  loaded the model through a Chinese symlink path, and correctly recognized the
  bundled short fixture in streaming mode.

The ready-to-run local bundle is `target/release/bundle/macos/Saymore.app`.
Build/test and measurement artifacts are under `target/qwen-streaming-port`;
visual artifacts are under `target/qwen-streaming-ui`. Windows CPU acceptance, a
full 20-minute stress run, and direct numerical comparison with vLLM remain
unverified. No runtime/model replacement or cloud fallback was introduced.
