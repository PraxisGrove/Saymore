# SeACo automatic download

The desktop SeACo card now offers **Download** as its primary action. It uses
the same queued downloader, pause/resume, cancellation, hash verification and
atomic installation as the other local models. **Import model** remains a
secondary action for the same pinned files; no Python export is required.
Installing files does not select the ASR provider. Selection still loads the
model and runs the built-in recognition self-test.

## Artifact choice

The previous locally exported FP32 graphs have no matching verified public URL.
The download uses the existing public quantized export from
[MoYoYoTech](https://huggingface.co/MoYoYoTech/speech_seaco_paraformer_large_asr_nat-zh-cn-16k-common-vocab8404-pytorch),
pinned to revision `45796913d5696c6334d8e5e18fc98cf4d063ad88`. The model ID is
`seaco-paraformer-zh-int8`. No compatibility branch for the previous FP32
artifact is retained.

The [manifest](../../crates/infra/src/model_installer/seaco_manifest.rs) pins
all six filenames, exact byte sizes and SHA-256 digests:

- `model_quant.onnx`: 345,139,149 bytes, dynamic INT8 matrix operations.
- `model_eb_quant.onnx`: 34,028,133 bytes. Despite its filename, inspection
  found no quantized operators; embedding inputs and outputs retain the FP32
  contract.
- `am.mvn`, `config.yaml`, `tokens.json`, `seg_dict`: identical hashes to the
  support files used by the previous FP32 integration.

Total transfer: **387,563,472 bytes** (387.6 MB / 369.6 MiB). The app downloads
directly from that pinned repository, without mirroring or repackaging its
weights. Repository metadata labels the upload MIT; that label is not a new
assertion about original checkpoint redistribution rights. Earlier upstream
license/provenance limitations remain in the
[native validation record](seaco-native-validation.md).

The third-party export is not established as a byte-for-byte derivation of the
previous checkpoint. Its graph contract and observable behavior were validated
rather than assuming equivalence from the name.

## Native validation

On Apple M4 using the production linked ONNX Runtime:

- The main graph accepts `speech` FP32 `[B,T,560]`, lengths INT32 `[B]`, and
  vocabulary embeddings FP32 `[B,N,512]`. It returns logits FP32 `[B,T,8404]`
  and token counts INT32 `[B]`, matching the existing adapter.
- The embedding graph accepts INT32 `[N,10]` and returns FP32 `[10,N,512]`.
- Pinned files load without executing repository Python or remote code.
- Bundled 3.859-second Chinese audio returns `可以通过语音进行人机交互`. One
  cold run: load 1.006 s, stop-to-ASR 0.150 s.
- The existing native vocabulary test passes baseline / hints / reordered hints
  / baseline and cancellation. The generated test fixture uses macOS Tingting
  reading `欢迎来到魔搭社区，达摩院的张伟正在介绍语音识别。`.
- On the previous 39.64-second continuous Chinese synthetic fixture, output
  matches the FP32 baseline exactly (normalized CER 1.12%). Single-run
  stop-to-ASR was 0.304 s versus the historical FP32 0.254 s. This is neither
  evidence of faster inference nor a broad accuracy guarantee.

Static ONNX checker 1.17.0 reports a topological-order error for an outer-scope
value referenced in a Loop subgraph. ONNX Runtime 1.20.1 and the production
runtime both resolve and execute the graph; production contract checks and
actual inference pass. Do not describe this artifact as passing static ONNX
checker. Native Windows inference has not been exercised here.

## End-to-end download verification

The production `benchmark_model_download seaco` entry point downloaded all six
files into a new installation root, verified hashes and activated the directory.
The installed files then passed both native recognition/vocabulary testing and
verified local re-import/load testing. Evidence is under
`target/seaco-download-review` (repository metadata, hashes, PCM and results),
with installer output in `/tmp/saymore-seaco-production-download.log`.

Relevant contract tests also cover downloaded files being imported unchanged,
installed files avoiding redundant network requests, bad imports preserving
existing files, resumable downloads and integrity failures. Desktop GUI tests
exercise the SeACo primary download and secondary import callbacks
independently.

## Verification status

- Infra installer/SeACo unit tests: 24 passed.
- Native tests on files installed by the production downloader: 2 passed.
- Desktop navigation and i18n tests passed, including independent download and
  import button callbacks.
- Infra and desktop all-target Clippy passed; size and semantic-color gates
  passed.
- Download and native validation used isolated directories and did not change
  the user's selected model or application settings.
