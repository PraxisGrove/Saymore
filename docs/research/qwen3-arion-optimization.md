# Qwen3-ASR optimization against Arion

This study precedes the
[production GGUF integration](qwen3-gguf-integration.md). Its ONNX baseline and
Python-worker results remain historical measurements; use the integration record
for the current Rust adapter and packaging acceptance.

## Objective and scope

Investigate whether Saymore's historical Qwen3-ASR latency can be improved using
Arion's implementation as a reference. Work stays on the current Saymore branch.
This is an engine feasibility study, not a production backend migration.

The promising first step is Qwen3-ASR **1.7B Q8_0 through speech.cpp with
Metal**. Keep the model size and existing scheduling initially, then evaluate
pause-aware segmentation and draft verification separately. Reducing the
checkpoint to 0.6B or reducing quantization precision is not necessary to test
this first step.

## Source identities

Inspected sources, pinned rather than relying on moving default branches:

- Saymore: `a9a45cbe444be4ccbea3ae6acc6447662ca5a0f9`.
- [Arion](https://github.com/AstraFoundry/Arion/tree/901b7553f116773f823c137fcd01b7f2ba9e2204):
  `901b7553f116773f823c137fcd01b7f2ba9e2204`.
- [speech.cpp v0.8.11](https://github.com/nyosegawa/speech.cpp/tree/c4bdcd0e751bd29460cb1353e6d54499828a1d90):
  `c4bdcd0e751bd29460cb1353e6d54499828a1d90`.
- Its pinned ggml submodule: `cbdeccfd49ec9c702c175c273d36c47520cf4710`.
- GGUF: `sakasegawa/Qwen3-ASR-1.7B-GGUF`, revision
  `81e2423d24073cdeeccb5951df16accc89e075a2`, file `Qwen3-ASR-1.7B-Q8_0.gguf`,
  2,176,109,216 bytes. Locally verified SHA-256:
  `5f219b78a1d9c3b9e97da27708b36f8a0bc1bfc1650b541c0a6dbaf87c9a62d0`. Source:
  [Arion model catalog](https://github.com/AstraFoundry/Arion/blob/901b7553f116773f823c137fcd01b7f2ba9e2204/models/catalog.json).

## What actually differs

| Aspect                   | Current Saymore                                             | Inspected Arion                                                                             |
| ------------------------ | ----------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| Checkpoint size          | Qwen3-ASR 1.7B                                              | Qwen3-ASR 1.7B                                                                              |
| Artifact/runtime         | INT8 ONNX, sherpa-onnx 1.13.4                               | Q8_0 GGUF, speech.cpp 0.8.11                                                                |
| Apple silicon compute    | CPU, four threads                                           | Metal / Accelerate                                                                          |
| Capture-time work        | Complete 30-second windows; 20 seconds with vocabulary      | Replaceable previews after one second of new audio                                          |
| Segment boundary         | Fixed window with one-second overlap and text deduplication | Pause midpoint after 800 ms below roughly -42 dBFS; quiet-point split for continuous speech |
| Maximum ordinary segment | 30 seconds without vocabulary                               | About 20 seconds during capture; 25-second bound when flushing                              |
| Reuse of earlier text    | Completed segments retained; new tail decoded independently | Previous preview of the same uncommitted segment supplied as `draft`                        |
| Runtime lifetime         | Loaded recognizer reused by desktop                         | Resident native subprocess with private stdio                                               |

Sources: Saymore [adapter](../../crates/infra/src/qwen3_asr.rs) and
[background scheduling](../../crates/infra/src/background_recognition.rs); Arion
[ADR 020](https://github.com/AstraFoundry/Arion/blob/901b7553f116773f823c137fcd01b7f2ba9e2204/docs/decisions/020-native-qwen-asr.md),
[recognizer](https://github.com/AstraFoundry/Arion/blob/901b7553f116773f823c137fcd01b7f2ba9e2204/apps/desktop/infrastructure/asr/recognizer.go),
[segments](https://github.com/AstraFoundry/Arion/blob/901b7553f116773f823c137fcd01b7f2ba9e2204/apps/desktop/infrastructure/asr/segments.go),
and
[worker lifecycle](https://github.com/AstraFoundry/Arion/blob/901b7553f116773f823c137fcd01b7f2ba9e2204/apps/desktop/infrastructure/asr/worker.go).

Arion previously used MLX affine 8-bit on Apple silicon. Its current ADR
explicitly retires that backend. Neither those historical MLX results nor
upstream M5 benchmarks establish the performance of the current speech.cpp
integration on our M4. The Go adapter launches speech.cpp with `--no-warmup`;
the worker supporting warmup does not mean Arion enables it.

These are still bounded offline recognition calls. Neither Arion's one-second
preview trigger nor speech.cpp's `draft` implements Qwen's native streaming ASR.
ONNX INT8 and GGUF Q8_0 are distinct quantizations; changing the backend
requires new model artifacts and independent quality verification.

## Mechanisms worth evaluating

1. **Metal inference first.** Saymore currently explicitly chooses `cpu`.
   speech.cpp supplies a compatible Qwen implementation on ggml/Metal. Its
   shared decoder selects flash attention when the GPU backend supports the
   operation. This requires a runtime and artifact change, not just changing
   Saymore's sherpa provider string. Source:
   [decoder backend selection](https://github.com/nyosegawa/speech.cpp/blob/c4bdcd0e751bd29460cb1353e6d54499828a1d90/src/common/qwen3-decoder.cpp).
2. **Pause-aware segmentation second.** A fixed 30-second cut can split a spoken
   phrase; an English continuation hallucination exists in the historical
   [continuous results](asr-latency-follow-up.md). Earlier natural boundaries
   can reduce pending work, but shorter context can also hurt recognition. Do
   not infer that the boundary caused that particular hallucination without a
   paired boundary experiment. Arion's energy threshold needs quiet-speaker and
   noisy-room testing before reuse.
3. **Draft verification with previews.** speech.cpp checks up to four proposed
   tokens per decoder step, accepts only the matching greedy prefix, and rewinds
   the rejected cache positions. The draft comes from the same segment's earlier
   recognition, not the reference transcript or another session. This is
   distinct from a vocabulary prompt, which steers recognition. Batched
   arithmetic can change near-tie token choices, so exact output equality is not
   guaranteed. Sources:
   [decoder](https://github.com/nyosegawa/speech.cpp/blob/c4bdcd0e751bd29460cb1353e6d54499828a1d90/src/families/qwen3-asr/decoder.cpp)
   and
   [draft batch size](https://github.com/nyosegawa/speech.cpp/blob/c4bdcd0e751bd29460cb1353e6d54499828a1d90/src/families/qwen3-asr/decoder.h).

One-second previews also add repeated frontend/encoder work. They should be
coalesced, obsolete requests cancelled, and stop-time tail preservation tested.
Copying that frequency onto the present CPU engine can increase contention.

## Local experiment

Release builds on Apple M4. speech.cpp was built from the pinned source with
Metal, embedded Metal shaders and Accelerate; the worker was explicitly launched
with `--device gpu --threads 4 --no-warmup`. Its ready message reports `MTL0`.
The current Saymore production `asr_latency_probe` was rebuilt with `--locked`.
Builds finished before timed inference started. No two benchmark engines run
simultaneously; ordinary desktop background applications remained active.

The comparison replays the original PCM fixtures at capture speed: short Chinese
3.859 s, continuous synthesized Chinese 39.644 s, and English 35.072 s. Both
paths load the model, recognize the first four seconds as warmup, then replay
the test. There are no vocabulary hints, network refinement, microphone capture
or target application insertions. Timing starts when replay stops and ends when
final text is ready; loading and warmup are excluded.

For the new backend, a research Python harness reproduces Saymore's 30-second
window, one-second overlap, serial background worker and boundary deduplication.
It invokes the real native worker through its documented private stdio protocol.
It does **not** use Arion's pause detector, preview loop or draft acceleration
in this comparison. Thus this is a feasibility comparison against the production
adapter, not a measurement of an integrated Saymore Metal adapter or Arion
itself. Runtime and artifact format change together.

Raw evidence and executable research harnesses are retained locally under
`target/qwen-arion-review/`: `compare.py`, `runs.jsonl`, `compare.log`,
`ablations.py`, `ablations.jsonl`, native stderr and build logs. These ignored
artifacts are not committed or guaranteed to survive a clean of `target/`.

### Stop-to-final results

| Fixture                      | Observations per engine | Current ONNX / CPU | speech.cpp / Metal |
| ---------------------------- | ----------------------- | ------------------ | ------------------ |
| Short Chinese, 3.859 s       | 3; median               | 1.340 s            | 0.416 s            |
| Continuous Chinese, 39.644 s | 1                       | 7.997 s            | 1.066 s            |
| Continuous English, 35.072 s | 1                       | 7.734 s            | 0.696 s            |

The short-sentence median decreases by about 69% (3.2 times faster); all six
short outputs are identical. The three Metal observations are 0.426, 0.416 and
0.407 s; the CPU observations are 1.340, 1.340 and 1.335 s.

For continuous Chinese, both engines have zero character errors on the 178-unit
normalized reference, and their complete outputs are identical. For English,
current ONNX gives 12 errors / 92 words (13.04% WER); Metal gives 2 / 92
(2.17%). The old inserted continuation about a stable network is absent from the
Metal output, but `network` is split into `net work`. This is a change on one
synthetic fixture, not general accuracy evidence. Scores remove punctuation and
lowercase; they do not assess formatting.

The current Chinese CPU observation is substantially slower than the historical
3.417 s. These runs are warmed, the historical continuous runs were cold, and
the host/software circumstances are not identical. The cause of the discrepancy
is not isolated here. Keep the historical data separate, and do not present the
single continuous observations as stable medians or promised speedup factors.

### Same GGUF on CPU

A separate worker using the same GGUF, `--device cpu --threads 4`, took a median
**0.946 s** over three warmed short-sentence decodes, versus the Metal
comparison's **0.416 s**. All outputs match. This control supports an
accelerator contribution (about 2.3 times faster for this short fixture) rather
than attributing the whole ONNX-to-GGUF change to Metal alone. CPU control
timings measure the native request round trip directly; for the short capture
replay the whole audio is likewise submitted only at stop, with negligible
scheduling overhead. It is not a continuous-speech CPU/GPU control.

### Draft verification

For each language, recognize the first 13 seconds of the original continuous
fixture, then use that actual output as the draft for its first 15 seconds.
Compare three warmed plain/drafted pairs, alternating order. These are complete
native request times, including audio encoding and IPC, not decoder-only or
capture-speed measurements.

| 15-second segment | Plain median | Draft median | Reduction |
| ----------------- | ------------ | ------------ | --------- |
| Chinese           | 1.319 s      | 0.731 s      | 44.6%     |
| English           | 1.462 s      | 0.796 s      | 45.5%     |

All six final outputs per language are byte-identical with and without the
draft. The initial prefix recognition itself costs 1.281 s for Chinese and 1.269
s for English. That work is excluded from the table: a recording-time preview
can hide it from the stop-time wait, but it is not free. The experiment does not
establish lower total compute, a production preview schedule, or equality on
other inputs.

### Verification and reproducibility

- Source build: Release speech.cpp library and CLI completed with the pinned
  ggml revision, Metal and Accelerate. Model size and SHA-256 match the pinned
  catalog.
- `cargo build -p template-infra --release --example asr_latency_probe --locked`
  passed; the comparison uses that rebuilt production executable.
- Ten capture-speed measurements completed: three short runs per engine, plus
  one run per engine/language for the two continuous fixtures.
- Fifteen supplementary timed requests completed: three same-GGUF CPU controls
  and twelve plain/draft requests; warmups and prefix construction are excluded.
- Each native response was checked for `type: end` and `stop: complete`; actual
  Metal device selection is recorded in the worker ready envelope.
- Reference scoring is saved in `target/qwen-arion-review/quality.json`.

After obtaining the pinned sources and model, native build commands are:

```sh
cmake -S target/qwen-arion-review/speech.cpp \
  -B target/qwen-arion-review/speech-build -DCMAKE_BUILD_TYPE=Release \
  -DGGML_METAL=ON -DGGML_METAL_EMBED_LIBRARY=ON -DGGML_ACCELERATE=ON
cmake --build target/qwen-arion-review/speech-build --target speech-cli -j 4
python3 target/qwen-arion-review/compare.py
python3 target/qwen-arion-review/ablations.py
```

Run those sequentially and only after all builds finish. The scripts overwrite
their result files, so preserve prior observations before repeating. No Rust
production source changed, and full Rust workspace test gates were not run for
this research-document change.

Completed: source comparison, verified artifact acquisition, native Metal build,
paired production/research replay, same-GGUF CPU control and draft verification.
Open: larger human/noisy/mixed-language corpus, repeated continuous timing,
pause-boundary experiment and production migration. The next implementation step
is an infra adapter for the pinned native runtime with model-installation and
packaging support, followed by the existing recognition-contract checks and real
end-to-end acceptance. No product backend has been replaced by this study.

## Integration boundaries and next step

A production migration belongs in `crates/infra`, implementing the existing
`StreamingSpeechRecognizer` port. Desktop continues to own runtime lifetime and
packaging, while the verified installer receives a newly pinned GGUF revision. A
C API binding or private native worker are both possible; the worker provides
process isolation and cancellation at the cost of packaging and IPC. Keep the
other models' existing ONNX runtime; Arion's wholesale removal of ONNX is
specific to its own model portfolio.

Before adoption, validate consecutive sessions, empty/quiet/noisy audio,
personal vocabulary and isolation, cancellation, shutdown, token-limit failure,
packaging and model relocation. Measure cold start, peak memory, power and
actual desktop stop-to-insertion separately. Windows needs its own CPU/Vulkan
inference and packaging evaluation. No cross-platform speed claim follows from
this M4 study.

Arion's application source is
[AGPL-3.0](https://github.com/AstraFoundry/Arion/blob/901b7553f116773f823c137fcd01b7f2ba9e2204/LICENSE).
The referenced
[speech.cpp runtime](https://github.com/nyosegawa/speech.cpp/blob/c4bdcd0e751bd29460cb1353e6d54499828a1d90/LICENSE)
is MIT; Qwen weights are Apache-2.0 according to the pinned catalog and upstream
model card. Prefer integrating the runtime directly and implementing the
application adapter within Saymore's architecture; this study imports no Arion
application source into the product.
