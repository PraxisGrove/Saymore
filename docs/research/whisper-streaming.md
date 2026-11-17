# Whisper live transcription with whisper.cpp

## Decision and primary sources

Keep the pinned `ggml-large-v3-turbo-q8_0.bin`, whisper-rs 0.16.0 /
whisper-rs-sys 0.15.0 (whisper.cpp 1.8.3), and Metal on macOS. Add a
provider-specific streaming choice alongside existing background segmentation.
No new runtime, model download, Python environment, SDL capture, or cloud
service is required. Non-streaming remains the default.

The upstream
[stream example](https://github.com/ggml-org/whisper.cpp/blob/v1.8.3/examples/stream/stream.cpp)
repeatedly calls `whisper_full` on a microphone window. Its defaults are a
3-second step, 10-second window and 200 ms retained audio; the
[README](https://github.com/ggml-org/whisper.cpp/blob/v1.8.3/examples/stream/README.md)
also demonstrates a 500 ms step and a VAD-triggered variant. The source
describes itself as a proof of concept and can discard audio when processing
falls behind. This is repeated full-window inference, not a causal Whisper
encoder.

[WhisperStreaming](https://github.com/ufal/whisper_streaming) is a different
Python project using LocalAgreement to confirm text across consecutive
hypotheses. Its README points to
[SimulStreaming](https://github.com/ufal/SimulStreaming), a newer torch-based
implementation. Their reported latency is not a measurement of Saymore's pinned
model or runtime. This integration uses the whisper.cpp window approach; it does
not claim to embed either Python project or reproduce its algorithm.

## Runtime and presentation contract

- Snapshot Whisper's `recognition_mode` and dictionary at recording start. The
  settings control persists independently of Qwen's mode and provider selection.
  Changes take effect on the next recording.
- Feed two-second audio chunks to a session-owned worker. Before another
  preview, consume queued chunks and keep every complete window. Slow inference
  coalesces obsolete previews without dropping audio.
- Re-read the current growing window for provisional text. Confirm each
  29-second window with the existing full-context decoder and release its audio.
  Keep the existing zero-overlap boundaries and text joining, including real
  repeated phrases; provisional text is replaced rather than appended.
- Preview calls use whisper.cpp's experimental `audio_ctx` option, with one
  second of padding and a cap at the model's 1,500 encoder positions. Each
  position covers 20 ms. Full windows and the final tail always use the original
  decoder parameters and full context. An explicit finish message bypasses
  obsolete preview work. A preview is never accepted as a final window merely
  because its audio length is unchanged.
- Keep automatic language detection, existing dictionary prompt tokens, greedy
  decoding, four threads and the existing Flash Attention setting. Every decode
  starts with fresh decoder state, so hints and hypotheses cannot cross
  sessions.
- Empty hypotheses clear the current provisional text. The overlay shows the
  latest 100 characters and checks session identity on the UI thread. Only the
  final transcript enters polishing and editor delivery.
- Bound total input/queued PCM to 30 minutes; each inference window is at most
  29 seconds. Errors fail explicitly. Cancellation discards queued work and
  suppresses late callbacks; it cannot interrupt an already-running native call.

The `audio_ctx` option is documented in the pinned
[whisper header](https://github.com/ggml-org/whisper.cpp/blob/v1.8.3/include/whisper.h)
and exercised by the stream example. The
[native full implementation](https://github.com/ggml-org/whisper.cpp/blob/v1.8.3/src/whisper.cpp)
performs automatic language detection before applying the audio-context
override. Consequently, reducing the preview context does not remove all
full-window work and cannot promise subsecond previews. Full-context
finalization protects the existing final decoding contract.

## Measurement and progress

Measurements use the production adapter, Release build, an Apple M4, warmed
model, capture-paced 16 kHz PCM, empty dictionary and disabled polishing.
Compilation is stopped while measurements run. Fixtures are macOS synthetic
speech, not a representative human corpus. First-partial times measure callback
arrival, not a rendered frame; callbacks arriving after stop are suppressed by
the recording UI. Stop-to-final excludes model loading and editor delivery.

The same-audio comparison and raw JSON are in
`target/whisper-streaming/comparison.json`. Short Chinese uses three runs per
mode; continuous Chinese/English use one run per mode. A full-context preview
baseline is retained under `target/whisper-streaming/full-context-previews`.
Results favor live feedback on longer speech, with additional work and a longer
wait after stop:

| Audio                          | Non-streaming first callback | Streaming first callback | Non-streaming stop-to-final | Streaming stop-to-final | Normalized errors, both modes |
| ------------------------------ | ---------------------------: | -----------------------: | --------------------------: | ----------------------: | ----------------------------- |
| Chinese, 3.859 s (median of 3) |           None before finish |     5.134 s (after stop) |                     2.914 s |                 4.138 s | CER 0/12                      |
| Chinese, 39.644 s              |                     32.610 s |                  3.617 s |                     3.137 s |                 4.597 s | CER 1/178                     |
| English, 35.072 s              |                     32.304 s |                  3.590 s |                     2.931 s |                 3.789 s | WER 0/92                      |
| Chinese, 118.931 s             |                     32.510 s |                  3.595 s |                     3.506 s |                 4.842 s | CER 6/534                     |

Final normalized text matches between modes on all fixtures. Chinese scoring
removes punctuation/spacing; English compares lowercased word tokens. The short
phrase ends before its streaming callback arrives, so the UI does not show a
live preview for that sample. Recognition quality here is a small synthetic
acceptance set, not a claim of general accuracy or streaming superiority.

The longer Chinese sample repeats the 39.644-second fixture three times and
crosses four window boundaries. `/usr/bin/time -l` reports peak process RSS
1,043.8 MiB (non-streaming) versus 1,089.6 MiB (streaming), and user+system CPU
time 1.04 s versus 4.91 s for the whole warmed probe process. GPU time and
energy are not measured. Input windows remain bounded rather than growing with
the recording. These measurements validate nearly two minutes, not 30 minutes.

Using full context for previews yielded first continuous callbacks around 4.9 s.
Shorter preview context reduced those to about 3.6 s but did not help the short
Chinese phrase. Full-context confirmation remains mandatory, and the
non-streaming default avoids this repeated preview workload.

An additional eight-second Chinese/English probe recorded the first actual
hypothesis at 3.208/3.193 seconds. English initially said "the experience of
Vogue" and finalized as "the experience of voice input". These provisional
errors reinforce the revisable-preview contract; evidence is in
`target/whisper-streaming/{zh,en}-preview-check.json`.

Verification:

- 573 workspace tests passed; 14 opt-in tests and four GUI binaries are excluded
  from the normal nextest run. Doctests passed.
- The real Q8_0 native test passed, covering pre-stop preview, full-context
  final equality, vocabulary, cancellation and session isolation.
- All four GUI executables passed their mandatory interaction assertions. The
  final local run occurred with macOS locked, so fresh snapshots were skipped.
  Tests now wait for a rendered frame instead of calling the OpenGL screenshot
  path on an uninitialized canvas; production overlay setup is reused.
- The size gate scanned 252 files with no warnings or errors.

The user also requested generated-file cleanup and a release. Cleanup removed
old debug query caches and desktop build artifacts: free disk space increased
from about 22 GiB to 97 GiB. Source, models and benchmark evidence were
retained. Default builds now use one Cargo job and no development/test
incremental cache. The completed full-test build sampled a peak process-tree RSS
sum of 4,010.8 MiB (2-second sampling; summed RSS is not unique physical
memory).

Version 0.2.8 local bundle construction and deep, strict code-signature
validation passed, including the final processing-label adjustment for
recordings that stop before a preview arrives. The final bundle build sampled a
peak process-tree RSS sum of 5,083.6 MiB. Local bundle signing is separate from
the Developer ID signing and notarization required in the Release workflow.
Repository hooks run before commit and push; GitHub CI and Release own the
remaining publication gates. Evidence is in
`target/whisper-streaming/{serial-acceptance,final-acceptance,bundle-final}.json`
and the corresponding logs.

```sh
cargo build --locked --release -p template-infra --example asr_latency_probe
SAYMORE_BENCH_COLD=0 SAYMORE_BENCH_WHISPER_MODE=streaming target/release/examples/asr_latency_probe whisper /absolute/model/directory apps/desktop/assets/asr-test/standard-zh.pcm capture disabled
# Repeat with SAYMORE_BENCH_WHISPER_MODE=non_streaming.
SAYMORE_WHISPER_MODEL_DIR=/absolute/model/directory cargo test --locked -p template-infra --test whisper_native -- --ignored
cargo test --locked -p saymore-desktop --test whisper_modes --test qwen_modes
```

Windows CPU performance and the full 30-minute bound are not yet measured.
