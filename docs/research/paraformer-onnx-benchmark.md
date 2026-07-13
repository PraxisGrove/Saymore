# Paraformer ONNX benchmark

## Scope

This benchmark compares the FP32 and Q8 exports of the local
Paraformer-zh-streaming model through the same native Rust/sherpa-onnx runtime.
It is a development benchmark, not the desktop application's production
adapter.

References and hypotheses are lowercased, then spaces, punctuation, and other
non-alphanumeric characters are removed. CER is the micro-average Levenshtein
distance over the remaining Unicode characters. A runtime failure is scored as
an empty hypothesis and is also reported separately.

## Data

| Split | Revision | Samples | Audio |
| --- | --- | ---: | ---: |
| [AISHELL-1 test](https://openslr.org/33/) via `shenyunhang/AISHELL-1` | `2724409d538167445e43ebf846990319f12a1cbf` | 7,176 | 10.03 h |
| [FLEURS `cmn_hans_cn` test](https://huggingface.co/datasets/google/fleurs/tree/70bb2e84b976b7e960aa89f1c648e09c59f894dd/data/cmn_hans_cn) | `70bb2e84b976b7e960aa89f1c648e09c59f894dd` | 945 | 3.07 h |

The downloaded data, generated manifests, predictions, and JSON reports live
under ignored `datasets/`. Model files live under ignored `models/`.

## Accuracy

| Split | Variant | CER | Character errors | Exact matches | Failures |
| --- | --- | ---: | ---: | ---: | ---: |
| AISHELL-1 | FP32 | 4.4586% | 4,671 / 104,765 | 4,472 | 0 |
| AISHELL-1 | Q8 | 4.4633% | 4,676 / 104,765 | 4,464 | 0 |
| FLEURS | FP32 | 13.2234% | 4,714 / 35,649 | 188 | 0 |
| FLEURS | Q8 | 13.2122% | 4,710 / 35,649 | 190 | 0 |

Q8 changes some individual predictions, but it does not show a meaningful CER
regression on either complete test split.

## Runtime

FLEURS was measured sequentially with four CPU inference threads on the same
machine. AISHELL timing is excluded because a controlled pause and a short
parallel-run experiment distorted elapsed inference time.

| Variant | Model files | Load | RTF | Peak RSS |
| --- | ---: | ---: | ---: | ---: |
| FP32 | 824.8 MiB | 0.85 s | 0.079 | 984 MiB |
| Q8 | 227.2 MiB | 0.48 s | 0.091 | 503 MiB |

Q8 reduces model storage by about 72% and peak memory by about 49%. On this
machine it loads faster but inference is about 14% slower, so quantization
should be selected for distribution size and memory rather than assumed CPU
speed.

## Reproduce

```bash
cargo build --release -p template-infra --example paraformer_benchmark

target/release/examples/paraformer_benchmark \
  models/paraformer-zh-streaming \
  datasets/aishell1/test.manifest.tsv \
  q8 \
  datasets/results/aishell-q8.json

target/release/examples/paraformer_benchmark \
  models/paraformer-zh-streaming \
  datasets/fleurs-cmn-hans-cn/test.manifest.tsv \
  fp32 \
  datasets/results/fleurs-fp32.json
```

The optional fifth argument limits the number of manifest rows for a smoke
test. Each run also writes a sibling `*.predictions.jsonl` file.
