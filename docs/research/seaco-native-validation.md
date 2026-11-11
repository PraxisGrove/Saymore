# SeACo native dictionary validation

Date: 2026-11-09. Objective: validate the exact user-specified SeACo model on
macOS Apple Silicon through a native runtime, including recognition-time
vocabulary, repeated sessions, latency and memory. Production Paraformer,
application settings and model installation are unchanged.

## Subsequent integration

On 2026-11-11, a separate user request authorized production integration. The
[local ASR expansion](local-asr-expansion.md) records the Rust implementation,
verified model import and application wiring. This document preserves the
original C++ feasibility experiment and its measurements.

## Outcome

SeACo runs locally through C++ FunASR + ONNX Runtime, without a Python inference
process. Recognition-time dictionary influence is demonstrated on a controlled
synthetic fixture:

- Spoken text: 欢迎来到魔搭社区，达摩院的张伟正在介绍语音识别。
- Empty dictionary: 欢迎来到摩哒社区达摩院的张伟正在介绍语音识别
- Dictionary `魔搭 / 达摩院 / 张伟`:
  欢迎来到魔搭社区达摩院的张伟正在介绍语音识别
- Empty dictionary again: 欢迎来到摩哒社区达摩院的张伟正在介绍语音识别

Moving `魔搭` to the last dictionary position preserves this correction and the
subsequent baseline restoration. The probe performs no transcript replacement or
LLM polishing. The changed text comes from native ASR decoding with embeddings.

This establishes a viable native dictionary mechanism, not general accuracy:
three other rare names did not improve in a separate synthetic fixture. Existing
streaming Paraformer remains a different model/backend without this mechanism.

## Implementation and provenance

The isolated [probe](../../crates/infra/examples/seaco_native_probe/README.md)
uses four CPU inference threads, batch size one, FP32 and greedy search. No VAD,
punctuation model, language model, ITN or GPU is enabled. Python is used only to
prepare/export/audit the model. It is absent from the C++ inference path.

Pinned source and model:

| Input                    | Revision/version                                                               |
| ------------------------ | ------------------------------------------------------------------------------ |
| FunASR native source     | `20f26b7c4905d010768ec91206149df2a90104b6`                                     |
| ONNX Runtime macOS arm64 | `1.20.1`                                                                       |
| ModelScope checkpoint    | `iic/speech_seaco_paraformer_large_asr_nat-zh-cn-16k-common-vocab8404-pytorch` |
| ModelScope revision      | `0141367fdc9b6ba58b0442ef34bceb56a6c1789c`                                     |
| Python exporter          | FunASR `1.2.6`, Torch/Torchaudio `2.5.1`                                       |
| Export dependencies      | Python `3.12.14`, ONNX `1.17.0`, NumPy `1.26.4`                                |
| Export format            | FP32 ONNX, opset 14, CPU, no quantization                                      |

| Artifact                 |     Bytes | SHA256                                                             |
| ------------------------ | --------: | ------------------------------------------------------------------ |
| FunASR source archive    |         — | `ba1c1d652f68565585e30fa1522bdb82a03056526d433419a4cb1843402f617e` |
| ORT arm64 archive        |   7878242 | `b678fc3c2354c771fea4fba420edeccfba205140088334df801e7fc40e83a57a` |
| Exact `model.pt`         | 989763045 | `3d491689244ec5dfbf9170ef3827c358aa10f1f20e42a7c59e15e688647946d1` |
| Exported `model.onnx`    | 957692158 | `20280dab4185fd35556f021e5c8d343d40e107bce1701afaa1e24654cc9f3dee` |
| Exported `model_eb.onnx` |  34026072 | `37fc62cb078a341617af90aca5a753e71c3acaee60cdabbc2c19000d45cfd938` |
| Official hotword WAV     |    253642 | `51792bc95be33075c1a8abb9afb76ad9f72943e84cd723cc8825b2678799b004` |

All 1095 loaded state tensors were checked against the exact checkpoint before
export. Loading enforced `weights_only`, with no remote model code. Both
exported graphs passed ONNX checker and actual ORT CPU execution. The embedding
graph ran with dynamic word counts 1, 3 and 6. Audio frontend and main graph
execution on the official recording returned finite outputs.

Do not substitute HF `funasr/paraformer-zh`: its checkpoint hash differs. The
official HF `funasr/SeACo-Paraformer-large` repository had no weights at
validation time. These ONNX files were exported locally from the exact
ModelScope weights.

The
[runtime patch](../../crates/infra/examples/seaco_native_probe/funasr-macos.patch)
fixes four isolated upstream compatibility problems: OpenFST's nonexistent copy
constructor field, the flat official ORT header layout, an Apple include guard
hiding a shared constant, and INT32/INT64 `token_num` decoding. All final
recorded comparisons were rerun after the decoder type correction. No production
Rust or Slint files were changed.

## Recognition-time contract

`CompileHotwordEmbedding` tokenizes each term, runs `model_eb.onnx`, and selects
the final valid token's 512-dimensional embedding. The native main graph
receives `speech [B,T,560]`, `speech_lengths INT32 [B]` and
`bias_embed [B,N,512]`. Embedding output is `[10,N,512]`. A blank/sentinel
embedding is included even for an empty dictionary, so counts include one extra
entry.

The graph's logits depend on the bias input. Its SeACo branch uses a NO_BIAS
hard gate: positions choosing that marker fall back to ordinary decoder logits.
Different dictionaries can therefore produce exactly identical outputs. No
additional hotword switch or external scale was required for this export.

The exporter constructs an attention length from the traced word count. For this
single-batch, unpadded path its all-zero mask broadcasts across the supplied
words; the reordered positive case confirms the last word can take effect.
Padded batches are outside this validation.

The upstream compiler truncates to ten tokens and skips unknown vocabulary;
`Saymore` was skipped in the standard fixture while `思源黑体` was accepted.
Whitespace separates terms, so English phrases need explicit design before
integration. Upstream logs print hotwords and must not become app diagnostics.

## Measurements and accuracy limits

Exact outputs, dictionary order, PCM hashes and timings are retained in
[acceptance-results.json](../../crates/infra/examples/seaco_native_probe/acceptance-results.json).
Every case uses baseline / dictionary / baseline on one model instance. All five
comparisons restored the baseline text after removing hints.

| Fixture                     | Audio seconds | Load seconds | Inference seconds: before / dictionary / after | Peak resident MiB |
| --------------------------- | ------------: | -----------: | ---------------------------------------------- | ----------------: |
| Official recording          |         7.925 |        3.268 | 0.230 / 0.208 / 0.277                          |              1492 |
| Rare names, Tingting        |        13.577 |        2.114 | 0.707 / 1.925 / 0.720                          |              1347 |
| Bundled standard Chinese    |         3.859 |        3.075 | 0.282 / 0.155 / 0.170                          |              1329 |
| 魔搭，Tingting              |         5.120 |        3.489 | 0.648 / 0.660 / 0.359                          |              1276 |
| Same audio, reordered words |         5.120 |        3.606 | 0.780 / 0.451 / 0.301                          |              1159 |

These are individual local measurements, not a stable comparative benchmark. The
load time includes native initialization; inference excludes embedding
compilation. Peak resident memory is cumulative for the process, not the model's
incremental allocation. CPU load, caches and system memory pressure varied. No
speedup can be attributed to adding hints from these runs.

The official recording produced
`国务院发展研究中心市场经济研究所副所长邓玉松认为` in all passes; the supplied
`魔搭 / 达摩院` terms were absent from its speech. Its fixed README did not give
a verbatim reference for this particular WAV. The standard fixture matched
`可以通过语音进行人机交互` in every pass with unrelated dictionary terms.

The rare-name fixture spoke:

> 今天，熵舟团队正在测试声澈引擎。请把弈澜项目的结果交给砺言科技。
> 熵舟、声澈、弈澜、砺言，都是这次测试的专有名称。

With and without those four terms, the model returned:

> 今天商周团队正在测试声澈引擎请把一兰项目的结果交给利研科技商周声澈一兰力研都是这次测试的专有名称

Only `声澈` was spelled as intended; the dictionary did not fix the other names.
The positive 魔搭 result and this negative case both use macOS Tingting
synthetic speech. Real-human proper-name accuracy, accents, long recordings,
large vocabularies, cancellation and streaming behavior remain untested.

## Scope and integration constraints

The prototype establishes macOS arm64 feasibility. It does not install SeACo in
Saymore, replace streaming Paraformer, alter the dictionary UI or package
models. The native API exposes C++ STL types; production Rust would need an
explicit C-compatible wrapper or a separate ORT implementation behind the app
port. Windows, packaged dynamic library loading and redistributable model
artifacts have not been validated.

The model card labels Apache-2.0, while the FunASR MODEL_LICENSE 1.1 text
contains reference/learning restrictions covering weights and derivatives.
Runtime source licensing does not resolve checkpoint redistribution rights. This
local validation neither mirrors weights nor establishes permission to bundle
them.

## Reproduction and progress artifact

Scratch root: `target/seaco-validation-np3tss9o`. It contains downloaded
archives, patched native source, `runtime-build`, `probe-build`, `fixtures`,
`models`, `export-env` and `own-scripts`. Large model/audio/build artifacts are
ignored. The probe README records native build and invocation commands. Retained
`export_exact.py` and `inspect_onnx.py` record export/audit logic; the scratch
`own-scripts/prepare_all.sh` and `export-env/requirements-freeze.txt` record
this run's preparation and resolved Python dependencies. Export requires the
verified `models/exact-ms/download-manifest.json` produced during preparation.

Verification completed: exact model hashes and state tensor audit, both ONNX
checkers, ORT dynamic embedding shapes, native Release build, standalone probe
build, and five native comparisons including an observable correction and a
negative accuracy case. Rust workspace gates are not required for this isolated
C++/Python research probe and were not rerun.

Current objective is complete. Changed files are this report and the isolated
probe directory. Remaining work for a separately authorized integration is the
adapter/model-installation contract, packaging, licensing resolution and broader
real-speech evaluation. No SeACo commit, push or release was requested.
