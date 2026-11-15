# MOSS Native Integration

Historical integration record: MOSS implementation and compatibility code have
since been deleted after continuous-speech latency testing. See
[latency follow-up](asr-latency-follow-up.md) for current status. The code
paths, commands and UI/lifecycle descriptions below refer to historical
revisions.

## Scope And Artifact Choice

MOSS-Transcribe-Diarize 0.9B is an experimental fifth local ASR option. It runs
natively in `crates/infra` using the same linked CPU ONNX Runtime as SeACo.
There is no Python process, cloud transcription request, or additional inference
DLL.

The earlier [feasibility study](moss-transcribe-diarize-feasibility.md)
recommended self-exported dynamic INT8 files, but those ignored research
artifacts and export scripts are absent from this checkout. This integration
instead pins soniqo's published FP16 ONNX conversion. It is substantially more
memory-hungry and is not a default recommendation for low-memory devices. The UI
identifies it as experimental, CPU-based, and requiring about 10 GB of free
memory.

- Upstream:
  [OpenMOSS-Team/MOSS-Transcribe-Diarize](https://huggingface.co/OpenMOSS-Team/MOSS-Transcribe-Diarize).
- Conversion:
  [soniqo/MOSS-Transcribe-Diarize-0.9B-ONNX-FP16](https://huggingface.co/soniqo/MOSS-Transcribe-Diarize-0.9B-ONNX-FP16).
- Conversion revision: `3110ffbd08d94d15bdf931c636284f042d21e3cc`.
- Converter-declared source revision:
  `e6d68cdfcddbdad1a7e8454f0cb859cad76e2502`.
- License: upstream and conversion model cards declare Apache-2.0. The app
  downloads directly from the converter's Hugging Face repository; Saymore does
  not publish or bundle the weights. The details page attributes both OpenMOSS
  and soniqo. Only graphs and tokenizer data are consumed; repository Python
  code and chat-template code are not executed.

| File                 |      Bytes | SHA-256                                                            |
| -------------------- | ---------: | ------------------------------------------------------------------ |
| `audio_encoder.onnx` |  625165991 | `07341e7e7b4271d34a4023e3dc204ec3912cfaa8840778a0827d06d6e130eb88` |
| `decoder.onnx`       | 1504146682 | `a00c0dde2828a6f6ead46531dee9a2c34e32afa67ed4e46042282a65dea6c447` |
| `tokenizer.json`     |   11423222 | `bcf03774334462d6e34b5005cb11120a62275f146ee2953e68731ecdbce84fbb` |

The download is 2,140,735,895 bytes (about 1.99 GiB). These are self-contained
ONNX graphs with no external tensor files. The existing downloader owns resume,
SHA-256 verification, atomic installation, and failure cleanup. Downloading does
not activate the model. Activation loads it and runs the bundled recognition
self-test before saving selection. Switching providers releases the previous
cache; an active model cannot be deleted. Storage accounting and history retain
the independent model/provider identities.

## Runtime Contract

- Input is mono 16 kHz signed 16-bit PCM. Recording is buffered until finish; no
  partial results are emitted.
- Non-overlapping segments of at most 30 seconds are decoded independently to
  bound prompt and KV-cache memory. This intentionally differs from upstream's
  single long-context meeting transcription. Cross-segment speaker identity and
  timestamps are not exposed. Speech at segment boundaries may need further
  accuracy evaluation. The existing 30-minute capture limit is also enforced at
  the adapter boundary.
- The Rust frontend implements the Whisper 400-point periodic Hann STFT,
  reflected center padding, 160-sample hop, 80 Slaney mel bands, and 30-second
  zero padding. Features are normalized before conversion to FP16.
- The pinned tokenizer is loaded as data. Prompt construction follows the
  upstream default Chinese transcription instruction, including 5-second numeric
  anchors only when the audio duration reaches the corresponding marker.
  Dictionary hints are snapshotted per session, deduplicated, restricted to 256
  UTF-8 bytes and 64 entries, and exclude control/special-token delimiters.
- The encoder consumes `[1,80,3000]` FP16 and returns `[1,375,1024]` FP16. The
  decoder uses FP16 embeddings, masks, logits and KV; IDs/positions are int64.
  Empty-cache prefill uses ORT's allocator constructor because the Rust
  binding's raw-data constructor rejects a zero-sized KV axis.
- Each segment has fresh KV state. New rows are appended within each layer/head;
  cache dimensions and finite outputs are checked. Greedy generation must reach
  a stop token within the duration-based budget or return an explicit error.
- Complete `[start][Sxx]text[end]` wrappers are removed. Unknown brackets and
  malformed wrappers are preserved rather than silently deleting text. Empty
  input, exact digital silence, and empty final text return errors. This is not
  a general speech-activity detector or a guarantee against noise
  hallucinations.
- Cancellation before finalization discards buffered audio without decoding. The
  current consuming recognition port does not interrupt an already-running
  `finish`; this follows the existing offline adapters.
- Shared ORT initialization is now in `ort_runtime.rs`. Native logs and raw ORT
  errors remain suppressed so paths, audio and vocabulary cannot enter logs.

New direct dependencies are `realfft` (already transitive; efficient Whisper
features), `half` (already transitive; FP16 graph tensors), and `tokenizers`
with only its `fancy-regex` feature (correct upstream Qwen byte-level BPE and
lookahead rules). ORT enables its existing `half` support. No engine version is
changed.

## Verification And Reproduction

The pinned files were retrieved into `target/moss-integration/artifacts` and all
three sizes and SHA-256 hashes were independently checked. Direct Hugging Face
connections were reset in this environment; test retrieval used hf-mirror.com
with the same fixed revision, then verified the bytes against the pinned hashes.
The production manifest retains the canonical Hugging Face source.

On Apple M4, the production adapter loaded in about 1.8 seconds and two runs of
the 3.859-second public Chinese fixture returned exactly
`可以通过语音进行人机交互。`, including with dictionary hints. The debug probe
reported about 7.0 and 8.2 seconds for inference. These are smoke-test timings,
not an idle-machine release benchmark and not the earlier INT8 study's results.

A release-build smoke test also returned the exact sentence twice, loading in
1.93 seconds and inferring in 6.29 / 6.24 seconds. `/usr/bin/time -l` reported
9,065,791,488 bytes peak RSS (about 8.44 GiB / 9.07 GB). This exceeds the
converter's reference profile; the UI therefore recommends about 10 GB free
memory and shows an 8.5 GB estimate using the application's binary-unit display
convention. This implementation does not use the converter's shared global CPU
arena; global arena retention across provider switching needs separate study.

The ignored native regression also passes consecutive plain/hinted sessions, a
33.859-second two-segment fixture, and exact digital silence. It uses only the
repository's public fixture and an explicitly supplied test model directory:

```sh
SAYMORE_MOSS_MODEL_DIR="$PWD/target/moss-integration/artifacts" \
  cargo nextest run -p template-infra --lib --locked --run-ignored only \
  -E 'test(moss_asr::native_tests)'

cargo run -p template-infra --locked --example moss_onnx_probe -- \
  target/moss-integration/artifacts apps/desktop/assets/asr-test/standard-zh.pcm \
  '可以通过语音进行人机交互。'
```

Deterministic tests cover frontend parity against independently generated
Transformers 5.23.0 / NumPy 2.5.4 features, KV layout, transcript parsing, hint
filtering, session isolation, segmentation, cancellation, silence, provider
persistence, storage accounting, activation/deletion protections, and real Slint
card/dialog download and selection callbacks.

## Checkout Validation

- `cargo check --workspace --all-targets --locked`: passed.
- `cargo nextest run --workspace --all-targets --locked`: 557 passed, 11
  skipped.
- Explicit `models_navigation` and `model_discovery_runtime` Slint executables:
  passed, including MOSS download and selection callbacks.
- Opt-in MOSS native regression: passed using the pinned artifacts.
- Workspace Clippy with warnings denied, doc tests, formatting, and semantic
  color checks: passed.
- `cargo deny check`: advisories, bans, licenses and sources passed; duplicate
  dependency-version warnings remain.
- `xtask size`: zero warnings and errors.
- Workspace release build: passed.

Windows packaged inference, GPU acceleration for MOSS, broad bilingual/noise
accuracy, hotword efficacy, and low-memory acceptance remain unverified. No
claim of parity with the original dynamic INT8 study or upstream BF16 accuracy
is made. GPU work remains a separate runtime/model compatibility task.
