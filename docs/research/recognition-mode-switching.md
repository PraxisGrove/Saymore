# Client-selectable recognition modes

Status: this is the earlier feasibility assessment. The user subsequently chose
the [official-algorithm native port](qwen-official-streaming.md), now
implemented for Qwen with a Streaming / Non-streaming selector. Whisper now also
has an independent selector using
[whisper.cpp window previews](whisper-streaming.md). The broader choices below
remain proposals.

## Finding

Saymore can offer a user choice of recognition strategy, but availability must
be scoped to the selected provider and installed runtime. A universal
`stream: bool` would conflate engine capability, when computation happens, and
whether interim text is displayed. This document is a proposal and feasibility
assessment; it does not add a client setting or change production recognition
behavior.

The old Qwen description is obsolete: the current adapter uses Q8_0 GGUF and
speech.cpp 0.8.11, with 30-second windows for all dictionary settings. The
20-second hinted ONNX path has been removed. The unpatched upstream runtime
explicitly
[does not implement qwen-asr's native streaming](https://github.com/nyosegawa/speech.cpp/blob/c4bdcd0e751bd29460cb1353e6d54499828a1d90/docs/models/qwen3-asr.md).
It can support a recording-time preview coordinator that submits complete,
growing windows and supplies the earlier reading as a draft. That is bounded
batch preview, not causal streaming inference.

The subsequent [official-streaming source review](qwen-official-streaming.md)
clarifies that Qwen's own streaming wrapper also re-feeds cumulative audio and
continues from a rolled-back text prefix. "Official streaming" must not be used
as a synonym for a causal encoder that computes only newly arrived frames.
Direct use of its vLLM API and a port to our native runtime are distinct
integration choices.

## Three independent concepts

1. **Engine:** native incremental recognition, or full-window inference.
2. **Scheduling:** run during recording, periodically re-read the uncommitted
   window, or defer audio processing until the user stops.
3. **Display:** show or hide interim hypotheses. Hiding text can keep all the
   compute and stop-latency benefits of the original schedule.

Local/offline network use is another independent property. A local model can
recognize incrementally without sending audio anywhere. ASR mode must not
silently change the model, choose a cloud service or change network consent.

## Proposed user choices

Use concrete labels under each model's recognition settings:

| Choice                               | Meaning                                                                 | Main trade-off                                                                            |
| ------------------------------------ | ----------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| 实时识别 / Live recognition          | Native online decoding, or explicitly described rolling-window previews | Earlier feedback; previews can revise. Rolling windows add repeated work.                 |
| 后台分段 / Background segments       | Process completed segments during capture; finish the remaining tail    | Predictable work and less stop-time backlog, but short utterances have no early text.     |
| 录完识别 / Recognize after recording | Buffer bounded audio and begin recognition when capture ends            | Less inference activity during capture, but more work after stop and more retained audio. |

Only show choices backed by a validated implementation. Separately provide
`显示识别预览 / Show recognition preview` where interim results exist. This
presentation switch must not pretend to select a different model graph or reduce
computation. The names above intentionally do not promise best accuracy or lower
energy. A model can remain loaded even when computation is deferred.

Keep the mode per provider and snapshot it when recording starts, together with
dictionary hints. Changes take effect on the next recording. Preserve the
currently selected model and mode on errors; unsupported requests should be
explicit rather than silently routed to another engine or service. Existing
working defaults remain until alternatives pass acceptance.

## Feasibility by provider

| Provider   | Live path                                                                                                                                 | Other choices and qualification                                                                                                                                                                                                                     |
| ---------- | ----------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Qwen3 GGUF | Add a rolling-preview coordinator around speech.cpp, with per-segment draft verification, pause boundaries and stale-preview cancellation | Current background segments remain the baseline. Deferred processing is already demonstrated by the benchmark wrapper. Direct use of Qwen's official streaming API requires its vLLM backend; a native algorithm port is a separate implementation. |
| Whisper    | Add rolling-window inference through whisper.cpp and stable/provisional text handling                                                     | Keep background/deferred paths. Preview cadence must follow measured inference time; frequent requests can accumulate. Do not include the rejected Flash Attention change in this work.                                                             |
| SeACo      | No validated live path for the pinned graph                                                                                               | Initially offer only background/deferred processing. Small-window preview would need its own boundary, quality and resource evaluation; another Paraformer model's streaming capability does not validate this artifact.                            |
| Zipformer  | Already has native online decoding and interim hypotheses                                                                                 | Hiding previews keeps the online engine. Feeding buffered audio after stop is possible locally, but it remains the same online transducer and is not a higher-context offline model.                                                                |
| Doubao     | Existing WebSocket adapter receives native service interim results                                                                        | A preview display choice is feasible. A genuine final-upload mode requires provider/API support and testing of rate, timeout, quota and finalization contracts; do not merely buffer and burst into the streaming connection.                       |

Sources: [current mode contracts](../product/asr-recognition-modes.md),
[ASR app port](../../crates/app/src/speech_recognition.rs),
[desktop selection](../../apps/desktop/src/asr_runtime.rs),
[Qwen requests](../../crates/infra/src/qwen3_asr/native.rs), and
[benchmark scheduling wrapper](../../crates/infra/examples/latency_support/scheduling.rs).
Arion's pinned
[preview coordinator](https://github.com/AstraFoundry/Arion/blob/901b7553f116773f823c137fcd01b7f2ba9e2204/apps/desktop/infrastructure/asr/recognizer.go)
illustrates one-second scheduling triggers, pause-midpoint commits and a
previous reading supplied as a draft; that interval is not a guarantee of
one-second recognition latency. Its implementation is AGPL; use the behavior as
a reference and implement within Saymore's existing boundaries.

## Is streaming faster?

It is usually earlier to the first useful text. It can reduce stop-to-final wait
by moving work into recording, provided inference keeps up. Neither fact proves
lower total computation, lower power or a faster final reviewed insertion.

Native streaming can reuse acoustic/decoder state. Rolling-window recognition
can recompute the same audio repeatedly. If a preview takes longer than the
request interval, submitting every interval builds a queue. Coalesce pending
previews, discard obsolete results and prioritize finalization. The recent
Whisper baseline took about 2.565 s to recognize the 3.859-second short fixture;
that does not justify promising a completed preview every second.

Earlier hypotheses have less future context and can change names, numbers and
sentence boundaries. Full-context recognition may help disambiguation but is not
universally more accurate. Deferred execution of the same segmented/online
engine does not grant it a different context window. A full-audio final pass can
revise live previews but adds stop-time work; final correctness and feedback
latency should be assessed separately.

Measure first useful preview, stop-to-final, stop-to-insert, CER/WER, omissions,
preview revision/flicker, recording-time CPU/GPU, peak memory and energy.
Existing low stop-to-ASR times do not include network polishing or target
insertion.

## Current Qwen scheduling check

Four serial Release replays on Apple M4 used the current production GGUF
adapter, the same model and audio bytes, identical 30-second/one-second-overlap
boundaries, a four-second decoder warmup, and no polishing. Each cell is one
observation:

| Fixture                      | During-capture calculation | Calculate only after stop |
| ---------------------------- | -------------------------- | ------------------------- |
| Short Chinese, 3.859 s       | 0.410 s                    | 0.407 s                   |
| Continuous Chinese, 39.644 s | 1.080 s                    | 3.774 s                   |

Values are stop-to-final latency. Both schedules produce identical text on each
fixture. The continuous during-capture run's first internal partial arrives at
32.804 s, showing that the current 30-second schedule is not an interactive
preview implementation. Short audio completes before any full window exists, so
both schedules decode at stop; the few milliseconds of difference are not a
meaningful speed ranking.

This isolates scheduling, not native streaming versus offline model accuracy. No
new rolling-preview implementation was benchmarked. Results and reproduction
script are in `target/recognition-mode-review/qwen-scheduling.jsonl` and
`run.py`. The earlier [Qwen draft experiment](qwen3-arion-optimization.md)
reduced final request time on two 15-second segments, but excluded the cost of
producing the prefix draft and did not test a full live coordinator.

## Implementation seams and acceptance

- `crates/app`: typed mode/capability rules and session settings; keep UI and
  native implementation details out of this layer.
- `crates/infra`: bounded deferred scheduling, Qwen/Whisper preview
  coordinators, provider-specific cancellation and finalization. A growing
  window's new result replaces its provisional text; it must not be appended as
  another committed segment. Merely changing the existing window constant to one
  second is not sufficient.
- `apps/desktop`: per-model controls, session snapshots and a real preview view.
  The current callback writes `recording_detail` in
  [recording_runtime.rs](../../apps/desktop/src/recording_runtime.rs), but the
  inspected Slint source only declares that property and does not render it. The
  recording overlay currently displays waveform/state controls. Recording
  metrics and status messages also write the same property, so a preview needs
  dedicated transcript state. Native interim results therefore do not establish
  a visible live transcript UI.
- Associate preview delivery with the active session and revision (or equivalent
  guards), so queued UI events cannot overwrite a cancelled or newer dictation.
  Committed and provisional text need distinct ownership.
- Qwen's current worker requests are serialized under one mutex and send no
  `draft` or `cancel`. Live preview needs a request-lifecycle change to
  interrupt obsolete work and receive terminal acknowledgements, rather than
  waiting on a growing FIFO of previews. The pinned worker supports draft
  verification and cancellation.
- Keep previews inside Saymore initially; polish the final transcript through
  the existing processor and insert exactly once after its validation. Writing
  and revising partial text in arbitrary target apps is a separate delivery
  feature. The LLM's `stream: false` is independent of ASR scheduling.

Acceptance must cover rapid stop/cancel/restart, no dropped/duplicated onset or
tail, no vocabulary leakage, bounded memory/backlog, silence/noise, technical
terms, mixed language and final-mode quality on identical audio. Report p50/p95
latency over repeated runs. The four observations above are feasibility
evidence, not sufficient acceptance for a new mode or a new default.

Recommended order: expose safe preview presentation for already-online
providers; implement Qwen rolling previews with draft verification; evaluate
Whisper's cadence and quality; retain SeACo's proven segmented path until a live
variant passes. Client switching is feasible, but each offered mode must
correspond to a real implemented and measured path.
