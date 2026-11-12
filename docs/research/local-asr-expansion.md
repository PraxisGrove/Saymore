# Local ASR expansion

Date: 2026-11-11. User request: integrate SeACo-Paraformer and support more
local ASR models. The expansion added three models; the user's subsequent
request retired the old streaming Paraformer, leaving six supported local
models: Whisper, Qwen3-ASR, SenseVoiceSmall, SeACo-Paraformer, Zipformer and
Moonshine. Historical identities remain readable and selection stays explicit.

## Old streaming Paraformer retirement

The old provider card, native adapter, pinned download manifest and obsolete
probes are removed. Its desktop punctuation configuration is also removed;
SeACo-Paraformer is independent and remains supported. Loading a selected old
provider clears ASR selection, persists the migration once and never activates a
cloud or replacement model. Inactive old configuration and historical labels are
preserved; saving an active retired identity/type is rejected. Its queued
download is discarded without deleting audio or model files.

Existing model and punctuation files remain accounted for in Settings. Explicit
cleanup handles old revisions and incomplete download/install/backup
directories, rejects symlink roots and preserves supported models. The storage
page displays cleanup errors. Tests use temporary directories and disposable
settings; this change does not run cleanup against the user's actual data.

## Model contracts

| Model                  | Intended use                            | Recognition-time dictionary           | Installation                          |
| ---------------------- | --------------------------------------- | ------------------------------------- | ------------------------------------- |
| SeACo-Paraformer FP32  | Chinese, offline final text             | Native hotword embeddings             | Import the exact verified ONNX export |
| Zipformer bilingual    | Chinese/English, native streaming       | Native per-stream transducer hotwords | Download pinned upstream files        |
| Moonshine Tiny English | Lightweight English, offline final text | Unsupported                           | Download pinned upstream files        |

SeACo and Moonshine buffer PCM and decode independent ordered 30-second
segments; they do not fabricate partial results. SeACo has no punctuation or VAD
model. Zipformer preserves real streaming partials, endpoint/final handling, and
input chunk boundaries. All consume signed 16-bit mono 16 kHz PCM.
Download/import never selects a provider. Activation verifies availability,
loads and performs recognition self-test before saving selection; failure
preserves the previous provider. The selected model cannot be deleted. Switching
providers releases the old cache. Restored recordings use the same controller
and dictionary snapshot.

## SeACo implementation and import

The [prior C++ native validation](seaco-native-validation.md) established a real
recognition-time correction. Production integration instead uses direct Rust ORT
bindings and a Rust frontend, avoiding a separate FunASR/Python deployment.

The frontend is 80-mel Hamming fbank, 25 ms windows, 10 ms hop, LFR 7/6 and
560-dimensional pinned CMVN. Model execution uses four CPU threads. Vocabulary
is NFC-normalized, trimmed and deduplicated by tokens, examining at most 512
input terms and keeping up to 64 known terms of at most ten tokens each. Chinese
is tokenized by character, English words/phrases through the pinned `seg_dict`.
Unknown, control-containing and overlong terms are omitted intact. Each session
owns its hotword rows and embeddings, including the empty-list sentinel. Shared
graph access is serialized; hints do not survive in the shared model.
Cancellation before final decoding performs no inference. No transcript
replacement or cloud fallback is introduced.

New dependencies are `ort = 2.0.0-rc.12` (std, alternative-backend, API 22) and
`kaldi-fbank = 0.1.0`. ORT reuses `OrtGetApiBase` from sherpa-onnx's existing
linked static runtime; it does not download or load another inference DLL.
Native logging is silenced. The release-candidate binding version is pinned; its
safe logger initialization avoids the callback lifetime bug observed in rc.10
during validation. The app does not depend on Python or execute model repository
code at runtime.

There is no verified upstream download URL for the exact two exported SeACo ONNX
graphs. The app therefore offers **Import model** rather than a download button.
Select a directory containing:

```text
model.onnx
model_eb.onnx
am.mvn
config.yaml
tokens.json
seg_dict
```

The [import manifest](../../crates/infra/src/model_installer/seaco_manifest.rs)
pins sizes and SHA256 hashes of all six files. Only these files are copied;
symlink artifacts and mismatched exports are rejected. The source stays intact.
Staged copies are rechecked before atomic activation, and invalid input cannot
replace an existing installation. This accepts the tested export, not arbitrary
Paraformer checkpoints or `.pt` files.

The original checkpoint revision is `0141367fdc9b6ba58b0442ef34bceb56a6c1789c`;
the app's artifact revision adds `-onnx-1.2.6` to distinguish the exported
format. Prepared files for this checkout are under
`target/seaco-validation-np3tss9o/models/onnx-fp32`. Export provenance,
input/output hashes and the original validation limits remain in the earlier
report. Import does not publish or mirror weights, and the previously recorded
redistribution-license ambiguity is unresolved.

## Zipformer and Moonshine provenance

[Installer manifests](../../crates/infra/src/model_installer/complementary_manifests.rs)
contain the exact file sizes/hashes and immutable upstream URLs.

- Zipformer:
  `csukuangfj/sherpa-onnx-streaming-zipformer-bilingual-zh-en-2023-02-20`,
  revision `98590b7ed6443e77b714204da2757d75e1a642f4`. Uses
  `encoder-epoch-99-avg-1.int8.onnx`, FP32 `decoder-epoch-99-avg-1.onnx` and
  `joiner-epoch-99-avg-1.onnx`, `tokens.txt`, `bpe.vocab` and the upstream model
  card (Apache-2.0 metadata). The all-INT8 combination repeated characters on
  the bundled Chinese fixture; FP32 decoder/joiner restored the expected
  sentence. Do not silently switch the manifest back to all-INT8 to save space.
- Moonshine: `csukuangfj/sherpa-onnx-moonshine-tiny-en-int8`, revision
  `bf2b762c076d8ea61e2af0b3851c9564fb77552e`. Uses `preprocess.onnx`,
  `encode.int8.onnx`, `uncached_decode.int8.onnx`, `cached_decode.int8.onnx`,
  `tokens.txt`, and the bundled MIT license (Useful Sensors, 2024).

Zipformer uses modified beam search, four active paths, hotword score 1.5 and
`cjkchar+bpe` modeling. Each stream gets up to 64 terms / 1024 UTF-8 bytes, with
at most 64 characters per term. ASCII English is uppercased to match the model
vocabulary. Terms containing controls or `/ : # @` are omitted so dictionary
text cannot become native hotword grammar or metadata; unsupported characters
are filtered before passing hints to the native parser, avoiding OOV logs. Hints
bias recognition without guaranteeing a spelling.

Moonshine's activation and manual recognition-test action use the synthetic
[English fixture](../../apps/desktop/assets/asr-test/README.md). The dictionary
page reports that Moonshine and SenseVoice cannot apply recognition-time
vocabulary. LLM polishing remains a separate optional feature.

## Screenshot candidates checked

The user also supplied five Hugging Face listings. Official cards were checked
on the same date; the listing's Audio-Text-to-Text tag alone is insufficient to
establish a usable desktop ASR backend.

| Listing                                                                                                            | Evidence and decision                                                                                                                                                                                                                                                                                                                            |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| [MOSS-Transcribe-Diarize](https://huggingface.co/OpenMOSS-Team/MOSS-Transcribe-Diarize)                            | 0.9B ASR, Chinese among 50+ languages, vocabulary hints, timestamps and diarization; strong functional candidate. Not integrated in this change. The 2026-11-12 [feasibility study](moss-transcribe-diarize-feasibility.md) found a native path through the linked ONNX Runtime. It requires conditions and is offline-only: no partial results. |
| [Apriel-Voice-27B-preview](https://huggingface.co/ServiceNow-AI/Apriel-Voice-27B-preview)                          | Real transcription capability, but a large server-oriented multimodal model. Official examples use BF16, Transformers/vLLM and multi-GPU serving. No documented native desktop route; excluded from this expansion.                                                                                                                              |
| [d1-Omni-2B](https://huggingface.co/LiquidAI/d1-Omni-2B) / [GGUF](https://huggingface.co/LiquidAI/d1-Omni-2B-GGUF) | Decision/classification model; card explicitly states it writes no transcript or speech. GGUF/native execution does not make it a transcription model. Excluded.                                                                                                                                                                                 |
| [MOSS-Music-8B-Instruct](https://huggingface.co/OpenMOSS-Team/MOSS-Music-8B-Instruct)                              | Music understanding and sung-lyric transcription, approximately 9.1B including the encoder. Spoken dictation capability is not established; excluded.                                                                                                                                                                                            |

No weights from these screenshot candidates were downloaded or added to the
application. No general accuracy ranking is inferred from unrelated benchmarks.

## Reproduce native acceptance

The SeACo vocabulary effect is checked through the production Rust adapter:

```bash
SAYMORE_SEACO_MODEL_DIR="$PWD/target/seaco-validation-np3tss9o/models/onnx-fp32" \
SAYMORE_SEACO_FIXTURE_PCM="$PWD/target/seaco-validation-np3tss9o/fixtures/moda.pcm" \
CARGO_BUILD_JOBS=1 cargo nextest run -p template-infra --lib --run-ignored only \
  -E 'test(native_dictionary_changes_decoding_and_restores_baseline)'
```

The full importer checks approximately 1 GB twice before loading the model.
Debug SHA hashing can exceed the default 60-second test budget; use the longer
CI test profile for this explicitly ignored acceptance:

```bash
SAYMORE_SEACO_MODEL_DIR="$PWD/target/seaco-validation-np3tss9o/models/onnx-fp32" \
CARGO_BUILD_JOBS=1 cargo nextest run --profile ci -p template-infra --lib \
  --run-ignored only -E 'test(imports_exact_seaco_export_and_loads_native_model)'
```

The complementary-model probe exercises the actual adapters with consecutive
sessions, input chunk variation, empty input and cancellation:

```bash
CARGO_BUILD_JOBS=1 cargo run -p template-infra --example complementary_asr_probe -- \
  target/complementary-asr-validation/zipformer-mixed \
  target/complementary-asr-validation/moonshine \
  apps/desktop/assets/asr-test/standard-zh.pcm \
  target/complementary-asr-validation/synthetic-english/english.wav \
  'The quick brown fox jumps over the lazy dog'
```

This probe prints disposable fixture transcripts; do not use private recordings.
Native acceptance established the SeACo baseline/hinted/baseline correction,
correct Zipformer Chinese with/without hints and chunk-size changes, and English
recognition by both Zipformer and Moonshine on synthetic and public test audio.
These are acceptance examples, not accuracy benchmarks or proof that arbitrary
vocabulary will improve.

## Verification scope

Before the subsequent retirement, confirmed expansion acceptance included 32
provider/settings tests, 21 installer/storage tests, 20 desktop
lifecycle/self-test tests, 6 SeACo logic/session tests and 11
Zipformer/Moonshine contract tests. The production SeACo adapter passed the
baseline/hinted/baseline and hint-order checks. Full verified import and native
model load passed in 67.468 seconds using the CI test budget; the initial
default 60-second run timed out. GUI navigation passed with real import,
download and selection callbacks and active-memory badge checks. The final
installer/storage/vocabulary run passed all 25 tests, including the complete
import after correcting the destination flush handle for Windows compatibility.
The SeACo model-size metric uses pinned artifact bytes rather than the zero-byte
network download size.

Workspace all-target strict Clippy, Rustfmt, whitespace, typography, Markdown
formatting and spelling checks passed. The size gate scanned 234 files with zero
warnings/errors; the semantic-color gate found no direct color literals in 35
Slint files. Final review confirmed the extracted existing provider and download
operations preserve their previous bodies, all new model identities are wired
through lifecycle/history/storage, and user-owned `AGENTS.md` edits are
retained.

Retirement verification reran all 146 application tests and 30 focused
settings-migration, storage and retired-file cleanup tests; all passed. These
cover clearing canonical and custom retired selections without fallback,
persisting the migration once, preserving inactive provider configuration, and
explicit cleanup of corrupt/partial files while retaining SeACo. The size gate
now scans 231 files with zero warnings/errors; the 35-file semantic-color gate
also passes. On the isolated ASR candidate, all 40 focused desktop lifecycle,
self-test, provider-card and history tests passed, as did the real
`models_navigation` GUI executable. The final retired-queue migration fixture
also passed its focused rerun. Unrelated work is preserved separately from the
ASR commit.

The initial dependency audit passed license, ban and source checks, but its
advisory gate failed on existing `libsqlite3-sys 0.35.0` through
`rusqlite 0.37.0` (RUSTSEC-2026-0459). After the user authorized pushing to
`main`, the dependency was upgraded separately to `rusqlite 0.41.1` and
`libsqlite3-sys 0.39.1` (SQLite 3.54.1). The audit then passed all four
categories. The driver's `fallible_uint` feature retains checked unsigned
history and usage reads without changing the schema or encryption format.

macOS native evidence does not establish Windows acceptance or public release
readiness. Large weights and scratch recordings remain under ignored `target/`;
no real user recordings, provider keys or user settings are used for acceptance.
The user subsequently authorized committing these ASR changes on `main` and
pushing them to `origin/main`. Model mirroring and release remain outside that
authorization.
