# Other local ASR backends after the Qwen GGUF switch

## Objective and decision

Qwen's native GGUF integration was committed as `d63eb40`. The follow-up asks
whether Whisper, SeACo and Zipformer can benefit from the same approach. This
review uses their current adapters, installed artifacts and upstream source. It
does not reinstate MOSS, which is retired.

Do not migrate the remaining models wholesale to speech.cpp. Whisper already
uses a native Q8_0 engine with Metal. Its Flash Attention experiment improves
latency but drops two additional Chinese characters on the continuous fixture,
so that candidate was not retained. SeACo and Zipformer have no implementation
in the pinned speech.cpp runtime; their current integer-quantized ONNX graphs
also need more than a provider-name change for CoreML acceleration.

## Model-by-model findings

| Model                       | Current path                                                                                           | Practical next step                                                                                                                                        |
| --------------------------- | ------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Whisper large-v3-turbo Q8_0 | Rust calls whisper.cpp 1.8.3 through whisper-rs 0.16.0; macOS requests Metal                           | Evaluate native compute/segmentation settings with quality controls. There is no equivalent CPU-to-Metal migration left to perform.                        |
| SeACo-Paraformer INT8       | Rust feature extraction and vocabulary embeddings; native ONNX Runtime CPU; background offline windows | If further acceleration is needed, evaluate a CoreML-capable runtime and compatible graph export while retaining the SeACo bias input and embedding model. |
| Zipformer bilingual         | Rust calls sherpa-onnx's C++ online transducer; CPU; INT8 encoder, FP32 decoder/joiner                 | Preserve native streaming and hotword beam search. Prioritize only after measuring recording-time CPU/power or a real latency problem.                     |

Sources: [Whisper adapter](../../crates/infra/src/whisper_asr.rs),
[SeACo runtime](../../crates/infra/src/seaco_asr/runtime.rs),
[Zipformer adapter](../../crates/infra/src/zipformer_asr.rs), and
[speech.cpp 0.8.11's supported models](https://github.com/nyosegawa/speech.cpp/blob/c4bdcd0e751bd29460cb1353e6d54499828a1d90/docs/models.md).
The latter supports Qwen3-ASR and FastConformer-family recognizers, but not
these SeACo or Zipformer architectures. A GGUF file alone does not supply a
model's frontend, graph operators, streaming state or decoder.

Separating inference into a resident process can isolate crashes, native library
versions and memory ownership. It adds IPC and packaging work and does not
itself establish faster inference. Qwen's measured gain came with a different
model implementation and Metal computation; all three other adapters already
call native inference libraries.

## Whisper Flash Attention experiment

The pinned Rust binding defaults `WhisperContextParameters::flash_attn` to
`false`. The bundled native whisper.cpp defaults the corresponding parameter to
`true`. Saymore currently inherits the Rust default. Sources:
[whisper-rs 0.16.0](https://docs.rs/whisper-rs/0.16.0/src/whisper_rs/whisper_ctx.rs.html)
and
[whisper.cpp 1.8.3](https://github.com/ggml-org/whisper.cpp/blob/v1.8.3/src/whisper.cpp).

The experimental change explicitly enabled Flash Attention only on Apple
silicon. It left model bytes, greedy decoding, automatic language detection,
vocabulary handling and 29-second background windows unchanged. Saymore does not
use DTW alignment, which the binding documents as incompatible with Flash
Attention.

Both baseline and candidate were compiled as Release executables and saved
separately. The comparison ran serially on Apple M4 after compilation ended,
replaying the original PCM at capture speed. Each run loaded the same Q8_0 file
and decoded the first four seconds as warmup; refinement was disabled. Timing
starts at capture stop and excludes loading, warmup, refinement and delivery.
Short-case order alternates between baseline-first and candidate-first.

| Fixture                      | Runs per configuration | Baseline, Flash Attention off | Candidate, Flash Attention on |
| ---------------------------- | ---------------------- | ----------------------------- | ----------------------------- |
| Chinese short, 3.859 s       | 3; median              | 2.565 s                       | 2.050 s                       |
| Chinese continuous, 39.644 s | 1 initial              | 2.690 s                       | 2.187 s                       |
| English continuous, 35.072 s | 1                      | 2.515 s                       | 2.049 s                       |

The short-sentence reduction is about 20%; all short outputs match. Continuous
English output also matches byte for byte. Chinese output changes:

- Baseline includes `接下来请先完成录音测试然后检查网络润色`.
- Candidate has `接下来请先完成录音测试检查网络润色`, dropping `然后`.
- Both already misrecognize `只` as `知` elsewhere. Thus Chinese CER rises from
  1/178 (0.56%) to 3/178 (1.69%). English WER remains 2/92 (2.17%).

Scoring removes punctuation, lowercases and uses Chinese characters / English
whitespace-delimited words, matching the earlier exploratory evaluation. It does
not score formatting. One synthetic recording per language cannot establish
universal accuracy, but the observed deletion prevents treating this as a
quality-neutral performance improvement.

The production edit was reverted exactly, and the normal Release probe rebuilt.
No experimental setting or compatibility branch remains in production. Separate
candidate and baseline executables remain under the ignored research directory.
A second paired Chinese replay reversed the order (candidate first), with no
compilation or other inference running. It reproduced both transcripts byte for
byte: the candidate again dropped `然后`, taking 2.190 s versus the baseline's
2.694 s. This confirms the regression on this fixture; it does not establish its
frequency on a general corpus.

## SeACo and Zipformer: verified acceleration barriers

A temporary Rust probe loaded the real installed SeACo model to initialize
Saymore's actual linked ONNX Runtime, then queried execution-provider
availability. Result:

```json
{ "coreml_available": false, "cpu_available": true }
```

This measures the runtime linked by Saymore, not a Python ONNX Runtime wheel.
The probe source was moved out of `crates/infra/examples` after use; no
production API was exposed to perform this inspection.

The installed graphs were parsed with ONNX 1.23.2, including nested subgraphs:

| Graph                       | Nodes  | DynamicQuantizeLinear | MatMulInteger | Loop |
| --------------------------- | ------ | --------------------- | ------------- | ---- |
| SeACo `model_quant.onnx`    | 13,229 | 294                   | 314           | 3    |
| SeACo `model_eb_quant.onnx` | 33     | 0                     | 0             | 0    |
| Zipformer INT8 encoder      | 4,186  | 172                   | 182           | 0    |

The inspected
[CoreML operator factory](https://github.com/microsoft/onnxruntime/blob/14a22c471aa384b6340980aa71c1f0ab5a9a8384/onnxruntime/core/providers/coreml/builders/op_builder_factory.cc)
does not register `DynamicQuantizeLinear` or `MatMulInteger`. Its supported
`MatMul` is a different operator. A CoreML-enabled runtime can potentially run
other supported subgraphs, but integer compute can remain on CPU; node counts
are not a timing profile and do not quantify offload or speedup. ORT graph
optimization and subgraph partitioning would need to be measured with the exact
replacement runtime and export.

The
[CoreML execution-provider documentation](https://onnxruntime.ai/docs/execution-providers/CoreML-ExecutionProvider.html)
also notes that dynamic shapes can reduce performance. Saymore's SeACo inputs
have dynamic audio lengths and vocabulary dimensions, and Zipformer requires
streaming cache state across calls. Re-exporting or converting a model must
preserve those contracts. The
[sherpa-onnx 1.13.4 provider implementation](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.4/sherpa-onnx/csrc/session.cc)
contains a compile-time CoreML branch and a CPU fallback when unavailable;
changing a provider string is insufficient evidence of GPU use.

A future SeACo experiment should first retain its `speech`, `speech_lengths` and
`bias_embed` inputs plus the vocabulary embedding graph, then measure actual
provider partitioning, recognition quality, memory and end latency. A plain
Paraformer engine does not establish support for SeACo's contextual bias path. A
future Zipformer experiment must retain the online encoder/cache interface,
decoder/joiner, modified beam search and hotword behavior. The current mixed
precision deliberately avoids syllable repetition seen with an all-INT8 export.

Existing exploratory stop-to-final results for these CPU adapters are already
small: SeACo INT8 about 0.304 s on continuous Chinese, and Zipformer about
0.052/0.016 s on Chinese/English. These are historical observations from the
[SeACo validation](seaco-automatic-download.md) and
[continuous-speech follow-up](asr-latency-follow-up.md), not new repeated timing
measurements. Recording-time CPU and power remain unmeasured here.

## Evidence, status and next step

Artifacts are local under `target/other-asr-review/`: `whisper-flash.jsonl`,
`whisper-quality.json`, `whisper-zh-confirm.jsonl`, baseline/candidate
executables, `whisper_flash.py`, `confirm_whisper.py`, `linked-providers.json`,
`onnx_backend_review.rs` and `onnx-operators.json`. These are ignored research
artifacts; the report preserves the findings if `target/` is cleared.

Completed: Qwen commit, inspection of all three active alternative adapters,
real linked-runtime provider query, installed-graph inspection, Whisper paired
latency/quality experiment, and restoration of its unchanged production default.
No new SeACo/Zipformer runtime or artifact was installed into the product.

Next: obtain a broader human/noisy/mixed-language corpus before adopting Whisper
Flash Attention; only pursue new SeACo/Zipformer exports if latency, power or
recording-time compute measurements justify the integration. No CoreML/GGUF
speedup is claimed for either model.
