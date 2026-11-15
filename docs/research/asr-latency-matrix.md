# ASR Latency Matrix

Latest status: the 960 executable scheduling cases and 20 smoke checks have
completed. Four refinement transport fallbacks are recorded separately from
successful refinement. MOSS and its compatibility code have been removed after
continuous-speech testing. The current harness supports four models; five-model
measurements are historical evidence from earlier commits. See
[latency follow-up](asr-latency-follow-up.md) for current findings, verification
and remaining live refinement/delivery blockers. Progress sections below retain
the earlier run history.

## Objective

Minimize stop-to-delivered-text latency without losing recognition or refinement
quality. This first harness measures through **text ready**, not actual desktop
history persistence, clipboard/paste, microphone capture or UI rendering. Do not
call it a completed desktop end-to-end benchmark.

## Matrix And Corpus

`scripts/asr-matrix.py` creates a complete serial, seeded-random matrix:

- Four local models: Whisper, Qwen3, SeACo, Zipformer.
- Capture-time submission and deferred submission to the same decoder.
- Light, standard and thorough refinement, plus disabled as a control.
- Cold decoder and warm decoder (four-second prefix warm-up).
- Short public Chinese fixture and a synthetic 63.859-second segmented fixture.
- Three repetitions by default: 384 independent process runs/planned cells.

Capture means native online recognition for Zipformer and background complete
segments for the other three. Deferred Zipformer is **not an offline model**; it
is the same online decoder fed after capture. Native incremental audio paths for
the three offline models are not implemented, not alternative measured modes.

The long fixture repeats the short phrase padded to 30 seconds twice, then adds
the short phrase. This is a scheduling corpus, not continuous-speech accuracy
acceptance. Continuous medium/long speech, English, noise and vocabulary cases
are still needed before selecting a fastest production configuration.

## Run

```sh
cargo build -p template-infra --example asr_latency_probe --release --locked
python3 -m unittest discover -s scripts -p 'test_asr_matrix.py'
SAYMORE_BENCH_WHISPER_DIR="$PWD/target/asr-matrix/models/whisper" \
  python3 scripts/asr-matrix.py --output target/asr-matrix/my-run --execute
```

Set `SAYMORE_BENCH_<MODEL>_DIR` for each available model. Directories must
contain the pinned artifacts accepted by production loaders. Missing
configuration is recorded as blocked, never as a zero-latency success. Omit
`--execute` for a plan. Output directories must be new, preventing accidental
evidence overwrite.

For live refinement explicitly set `SAYMORE_BENCH_LLM_URL`,
`SAYMORE_BENCH_LLM_MODEL`, optional `SAYMORE_BENCH_LLM_KEY` and
`SAYMORE_BENCH_LLM_PROFILE` (default `portable`). This sends synthetic
transcript text to that endpoint and may incur charges. No saved user settings
or keychain credentials are read. Never commit keys. All three strengths use the
real app `FinalTextProcessor`, including short-text skipping, rejection and
fallback. Inspect its recorded status: a skip or fallback is not successful
polishing. Each process has an isolated circuit breaker; cross-dictation
provider failure pauses and reuse of a warm HTTP connection are not represented
yet.

## Evidence

Each JSONL row records model/path/strength/fixture/repetition, cold/warm state,
fixture hash and status. Successful rows include model load, first partial, ASR
stop-to-final, refinement duration/status, stop-to-text-ready, capture-to-text
ready and transcript/output. Process wall time includes setup, loading and
warm-up and must not be confused with user-visible latency. Stage durations must
not be added across overlapping capture and decoding. Model load is reported
independently; cold does not mean the OS filesystem cache is flushed.

Metadata records revision, dirty-tree status, binary hash, platform, random seed
and LLM model/profile. Credentials and provider stderr are not persisted. Runs
are serial to avoid artificial simultaneous model contention; each has a
600-second timeout. Results are flushed after each cell so interrupted runs
retain completed evidence. A failed process currently records only its failure
category/exit code; stage diagnostics for failed requests remain follow-up work.

## Independent Session Scheduling

The benchmark now also supports `--scheduling during-capture finish-only`. This
doubles executable scheduling coverage independently of PCM submission.
`finish-only` buffers in a benchmark session wrapper until `finish`, then sends
ordered PCM to the same decoder; `during-capture` uses the existing adapter.
This explicitly compares when inference starts, not alternative model graphs.
Zipformer's finish-only case still uses its native online transducer internally.
Native decoder mode remains separately recorded; the unsupported-mode audit must
not be counted as executed inference.

Probe override: `SAYMORE_BENCH_SCHEDULING=finish-only`. The default retains
existing behavior. A unit test verifies no early decoder input in finish-only
mode and exact sample order after finalization. The historical short-speech
cross-check ran all five models, both schedules and both submission paths (20
actual runs), with refinement disabled and warm decoders. Evidence is under
`target/asr-matrix/scheduling-cross-check`. The existing long matrix is paused
at a case boundary for this check and automatically resumed in a finally block.
Full long-speech/refinement repetitions for the new scheduling dimension remain
to be run; these smoke checks alone are not complete acceptance.

## Decoder Mode Coverage

Use `--audit-modes` to enumerate decoder mode independently of PCM submission:
768 cells with three repeats. Each row now records `inference_mode` even in the
existing 384-cell mode. Zipformer uses native online decoding; the other three
use offline graphs with background segment scheduling. The opposite decoder mode
is marked `unsupported` with `decoder_mode_not_implemented_for_pinned_artifact`.
This audit does not add a native streaming implementation or double actual
executable coverage. Existing running processes retain their original schema.

An additional isolated native smoke run completed for both previously queued
models, pausing matrix scheduling until the current child finished, then
resuming it after four measurements. Short Chinese, warm decoder, refinement
disabled:

| Model     | Capture submission stop-to-ASR | Deferred submission stop-to-ASR |
| --------- | ------------------------------ | ------------------------------- |
| Whisper   | 2.186 s                        | 2.150 s                         |
| Zipformer | 0.033 s                        | 0.203 s                         |

All four transcripts were `可以通过语音进行人机交互`. Raw JSON is in
`target/asr-matrix/priority-native`. These are one-run smoke measurements of
each existing decoder, not streaming/offline mode comparisons. Four matrix
framework tests pass, including independent mode/submission enumeration.

## Full Model Matrix In Progress

All five model artifact sets are now available locally. Qwen3 was downloaded
with the production verified installer; SeACo was reconstructed with all six
manifest hashes matching (see its native-validation record). Whisper and
Zipformer downloads initially failed on interrupted CDN connections. Downloading
independent 4 MiB ranges with retries recovered both pinned revisions. Whisper's
874,188,075-byte file and all six Zipformer files passed their original SHA-256
checks. Production manifests and model identity were not changed.

Three repetitions are running serially: MOSS/Qwen3/SeACo in
`target/asr-matrix/multimodel-repeat-3` (288 cells), followed by queued
Whisper/Zipformer in `target/asr-matrix/whisper-zipformer-repeat-3` (192 cells).
The latter starts only after the former has written all 288 rows, avoiding
concurrent inference. Both use the explicitly authorized DeepSeek configuration;
keys are process environment only, not stored in tracked scripts or results.
Artifacts are ready, but native execution and full matrix completion must still
be checked; download success alone is not a benchmark result.

## Progress

Initial implementation is in the infra example and the repository benchmark
script; production paths are unchanged. First real matrix run completed with
available MOSS CPU weights and one repetition: 160 cells, 8 measured and 152
blocked (missing other model directories or explicit LLM configuration). Raw
evidence: `target/asr-matrix/run-1/{metadata.json,runs.jsonl}`.

Stop-to-text-ready seconds, refinement disabled:

| Fixture           | Decoder | Deferred | Capture |
| ----------------- | ------- | -------- | ------- |
| Short Chinese     | Cold    | 6.230    | 6.204   |
| Short Chinese     | Warm    | 6.330    | 6.298   |
| Segmented Chinese | Cold    | 26.042   | 11.894  |
| Segmented Chinese | Warm    | 22.284   | 9.965   |

These are single-run observations, not medians or tail latency. No live
refinement calls were made. Framework checks: three Python tests, release probe
build, example Clippy, Rust formatting and size gate passed.

Next: supply remaining model directories and an approved benchmark LLM endpoint,
expand the public speech corpus, run three or more repetitions, aggregate
medians/tail latency only among matching successful statuses, review output
quality, and instrument the desktop delivery boundary. Until then the matrix is
partial and cannot establish the best full-pipeline configuration.
