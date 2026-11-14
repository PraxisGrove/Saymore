# Background ASR Device Latency

## Objective And Implementation

Reduce waiting after recording without changing the user's chosen model,
artifacts, vocabulary snapshot or existing segmentation rules. The infra
`background_recognition` worker now starts complete windows during capture for
Qwen3, Whisper, SeACo and MOSS. Zipformer retains its online implementation.
This does not implement upstream Qwen streaming or native incremental MOSS.

## Device Measurement

Apple M4, release build, pinned MOSS FP16 ONNX CPU artifacts. Each process loads
and warms the decoder with up to four seconds before measuring. The probe feeds
16 kHz mono PCM in 100 ms chunks at capture speed. No microphone or user
settings are accessed. Model loading is excluded from stop-to-final latency.

| Fixture                               | Before stop-to-final | After stop-to-final | Result                  |
| ------------------------------------- | -------------------- | ------------------- | ----------------------- |
| Public Chinese test, 3.859 s          | 7.130 s              | 7.558 s             | No short-utterance gain |
| Synthetic segmented Chinese, 63.859 s | 24.241 s             | 10.009 s            | 58.7% less waiting      |

Final text matched byte-for-byte in both comparisons. Short text was
`可以通过语音进行人机交互。`; long text repeated that sentence three times. The
long fixture is the public short fixture padded with silence to 30 seconds,
repeated twice, then followed by the short fixture. It tests scheduling, not
continuous conversational accuracy. First partial appeared at 42.105 seconds in
the changed long run; the baseline emitted none. Each table entry is one run,
not a statistical benchmark or a universal speed claim.

Reproduce after creating that fixture:

```sh
cargo build -p template-infra --example asr_latency_probe --release --locked
target/release/examples/asr_latency_probe moss \
  target/moss-integration/artifacts target/asr-latency-review/long-zh.pcm capture
```

The probe also accepts `buffered` to defer submission until capture ends, and
`whisper`, `qwen3`, or `seaco` with their corresponding installed model
directory. The actual before measurements used a preserved pre-change release
binary. Raw local JSON and that binary are in `target/asr-latency-review/` (not
tracked).

## Verification And Remaining Work

- Workspace tests: 563 passed, 11 skipped.
- Infra all-target Clippy and desktop all-target compilation passed.
- Worker tests cover pre-finish execution, nonblocking capture, original window
  boundaries and overlap, partial delivery, cancellation and decoder errors.
- Size gate: no warnings or errors.
- Whisper weight download failed with a broken connection; Whisper GPU/native
  speed remains unmeasured. Qwen3 and SeACo native speed are also unmeasured.
- This is real local inference with paced PCM, not packaged-app microphone or
  GUI latency. Continuous speech, multilingual/noisy speech, repeated timing
  trials, power and peak-memory comparisons remain follow-up work.
- Native calls already in progress cannot be interrupted on cancellation. A
  cancelled call can briefly delay a subsequent session using the same model.
- Short dictation still waits for finish and is not made faster by this change.

Native MOSS regression and desktop navigation/i18n checks also passed. Next
step: validate additional model artifacts when available; do not label any model
globally “best” or “fastest.”
