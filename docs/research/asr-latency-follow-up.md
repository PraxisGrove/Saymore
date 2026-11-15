# ASR Latency Follow-up

## Objective and order

Preserve capture-time inference, investigate polishing latency, diagnose MOSS,
then extend continuous speech and delivery validation. Work remains on main. The
user subsequently authorized retiring MOSS if its latency cannot be made
acceptable, then explicitly required removing compatibility code. MOSS is now
fully removed rather than retaining its adapter or migration branches. The small
thread-count experiment below remains historical evidence.

## Capture-time inference

Existing production scheduling remains enabled. Six background-worker contract
tests pass: full windows start before finish without blocking capture, ordered
overlap/tails, interim results, cancellation, errors, and input validation. The
completed 960-row scheduling comparison is in `target/asr-matrix`:
`multimodel-repeat-3`, `whisper-zipformer-repeat-3`, `finish-only-repeat-3`. All
rows measured; 716 refinements completed, 240 disabled, four transport
fallbacks. Those fallbacks are excluded from successful refinement comparisons.
These compare scheduling, not alternative native model graphs.

## Refinement

Desktop `RefinementRuntime` already caches `FinalTextProcessor` by settings,
reusing its reqwest client and Tokio runtime. The previous ASR probe creates a
new process/client for each case. Its roughly 0.7-second refinement medians do
not measure a reused connection across dictations.

Added `refinement_latency_probe` for six alternating fresh/reused client pairs
through the real processor, using a caller-owned UTF-8 transcript file and only
explicit `SAYMORE_BENCH_LLM_*` settings. The first reused request is a cold
connection and must be separated when aggregating. Local HTTP/1.1 verification
completed 12 requests: reused client used one TCP connection, fresh clients used
six. This verifies reuse, not live-provider latency improvement.

`llm.response_timing` reports response-header wait and body-read milliseconds.
Header wait includes connection setup, request transit and server generation; it
cannot independently identify network versus server compute. Diagnostics contain
only timings/status, never URLs, credentials, bodies or transcripts. Benchmark
diagnostics filter exclusively on `saymore::diagnostics`.

Live DeepSeek reuse comparison remains blocked: no benchmark credential is
available in this session environment. No production prompt, timeout, provider
or strength was changed without a demonstrated improvement.

## MOSS diagnosis and retirement

Historical diagnostic builds recorded `asr.moss.segment_timing`: frontend, mutex
wait, audio encoding, generation, and token count, without transcript/audio.
Enable in the replay probe with `SAYMORE_BENCH_DIAGNOSTICS=1` (stderr only).

Three cold short-Chinese runs per thread configuration on Apple M4:

| Encoder / decoder threads | Median stop-to-ASR |
| ------------------------- | ------------------ |
| 4 / 4                     | 5.976 s            |
| 8 / 8                     | 6.144 s            |
| 8 / 4                     | 5.766 s            |

All nine outputs match. Four-thread encoding was about 3.51 s and generation
2.42–2.52 s. Eight-thread encoding was about 3.30 s, but eight-thread generation
slowed to 2.81–2.88 s. The 8/4 change offered only about 3.5% improvement.
Evidence: `target/asr-matrix/moss-thread-review`.

Continuous synthesized Chinese/English exposed a larger problem: first-segment
generation took 53.53/83.07 s for 296/292 generated tokens. Encoder time was
4.23/5.05 s. This is accumulated generation work, not waiting on the session
mutex. These exploratory runs overlapped desktop compilation in part and are not
controlled latency comparisons. Nevertheless the short padded fixture was
plainly inadequate as evidence for continuous speech suitability.

The thread tuning was not retained. Following the user's direction to avoid
legacy compatibility code, MOSS implementation, model manifest, product/UI
branches, dedicated tests and exclusive dependencies have been deleted.
MOSS-specific migration and retired-storage branches have also been removed.
Historical measurements remain in this report; the current benchmark supports
only Whisper, Qwen3, SeACo and Zipformer. Existing local research artifacts were
not modified; reproducing old MOSS runs requires the historical code revision.

## Continuous speech and delivery

Added two local synthesized fixtures using macOS Tingting (200 words/minute) and
Samantha (180 words/minute), resampled with ffmpeg to mono 16 kHz i16 PCM. They
are exploratory synthetic fixtures, not a human speech acceptance corpus.
References, audio, raw transcripts and stage logs are retained under
`target/asr-matrix/continuous`; Chinese is 39.64 s, English is 35.07 s. One cold
capture-time run per model/language, refinement disabled:

| Model                   | Chinese CER | English WER |
| ----------------------- | ----------- | ----------- |
| SeACo                   | 1.12%       | 9.78%       |
| Zipformer               | 0.56%       | 6.52%       |
| Whisper                 | 0.56%       | 2.17%       |
| Qwen3                   | 0.00%       | 13.04%      |
| MOSS (experimental 8/4) | 0.56%       | 4.35%       |

CER removes punctuation/spacing and lowercases; WER compares whitespace words
with punctuation removed/lowercased using Levenshtein distance. Scores therefore
do not evaluate punctuation or formatting. One synthetic sample per language is
insufficient to select a default model or establish comparative accuracy.

Desktop `delivery.completed` now records elapsed milliseconds for platform
insertion/verification and optional clipboard preservation. This is a delivery
stage duration, not a shared stop-recording-to-insertion timeline.
`macos_text_delivery_probe` invokes the actual native deliverer only after
checking Accessibility permission and a dedicated foreground TextEdit document
named `Saymore Delivery Benchmark`. Permission was available, but preparing the
TextEdit document via Apple Events timed out; the probe rejected the unprepared
target without delivering text. Status:
`target/asr-matrix/delivery-status.json`. Actual native delivery latency remains
unmeasured.

## Verification and next steps

- Background contracts: 6 passed.
- Infra background/MOSS/provider unit tests: 16 passed.
- MOSS native consecutive sessions/long input/silence: 1 passed.
- Desktop completion/refinement tests: 13 passed before retirement changes.
- Retirement, settings migration, storage tests: 33 passed.
- Infra and desktop all-target Clippy passed after retirement changes.
- Release replay, refinement and native delivery probes built successfully.
- Retirement desktop unit tests: 35 passed; models-navigation GUI test passed.
- Formatting, whitespace/diff checks and size gate passed (zero warnings).
- Final removal: all 553 workspace tests passed (10 skipped), four Python matrix
  tests passed, semantic color and size gates passed. Source/UI/benchmark code
  contains no MOSS integration or MOSS-specific compatibility branches.
- Historical five-model results remain evidence only; current executable
  coverage is four models × two schedules × two feeds × four refinement modes ×
  two fixtures × cold/warm × three repetitions = 768 cases.

Remaining: run real DeepSeek reuse pairs with explicit benchmark credentials;
obtain human continuous/noisy speech with references; complete native delivery
measurement once TextEdit automation is functional; add a single end-to-end
stop-to-verified-insertion timeline. Do not claim these as completed.
