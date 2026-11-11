# SeACo native validation probe

Experimental C++ acceptance probe, outside the Cargo/application dependency
graph. It exercises FunASR's offline frontend, hotword compiler, ONNX Runtime
inference, and greedy decoding on macOS Apple Silicon. Use only disposable,
non-sensitive fixtures: both the JSON report and upstream runtime logs can
contain vocabulary and transcripts. This is not a production adapter.

## Pinned inputs

- FunASR runtime source: `20f26b7c4905d010768ec91206149df2a90104b6`.
- Official ONNX Runtime macOS arm64 binary: `1.20.1`.
- ModelScope model:
  `iic/speech_seaco_paraformer_large_asr_nat-zh-cn-16k-common-vocab8404-pytorch`.
- Model revision: `0141367fdc9b6ba58b0442ef34bceb56a6c1789c`.
- Exporter: PyPI FunASR `1.2.6`, Torch/Torchaudio `2.5.1`, ONNX `1.17.0`, NumPy
  `1.26.4`, Python `3.12.14`; CPU FP32, opset 14, no quantization.

Keep downloaded archives in separate empty directories; validate archive members
before extraction. Only the runtime subtree is needed from the FunASR archive.
Keep your scripts separate from downloaded source and run Python with `-I` from
the project root. Do not enable remote model code.

## Native build

Prerequisites: Xcode Command Line Tools and CMake. From the project root, define
absolute paths to the extracted runtime source, ORT distribution and build
roots:

```bash
export FUNASR_ROOT="$PWD/target/seaco-validation-np3tss9o/funasr-source/FunASR-20f26b7c4905d010768ec91206149df2a90104b6/runtime/onnxruntime"
```

```bash
export ONNXRUNTIME_DIR="$PWD/target/seaco-validation-np3tss9o/onnxruntime-source/onnxruntime-osx-arm64-1.20.1"
```

```bash
export FUNASR_BUILD="$PWD/target/seaco-validation-np3tss9o/runtime-build"
```

Apply the patch once to a pristine copy of the pinned runtime. It fixes an
OpenFST copy constructor, Apple include layout/guards, and the decoder's
incorrect assumption that `token_num` is always INT64 (this export returns
INT32).

```bash
patch -d "$FUNASR_ROOT" -p1 < crates/infra/examples/seaco_native_probe/funasr-macos.patch
```

```bash
cmake -S "$FUNASR_ROOT" -B "$FUNASR_BUILD" \
  -DCMAKE_BUILD_TYPE=Release -DCMAKE_POLICY_VERSION_MINIMUM=3.5 \
  -DONNXRUNTIME_DIR="$ONNXRUNTIME_DIR" -DENABLE_FFMPEG=OFF -DGPU=OFF
```

```bash
cmake --build "$FUNASR_BUILD" --target funasr --parallel 2
```

```bash
cmake -S crates/infra/examples/seaco_native_probe \
  -B target/seaco-validation-np3tss9o/probe-build \
  -DFUNASR_ROOT="$FUNASR_ROOT" -DFUNASR_BUILD="$FUNASR_BUILD" \
  -DONNXRUNTIME_DIR="$ONNXRUNTIME_DIR"
```

```bash
cmake --build target/seaco-validation-np3tss9o/probe-build --parallel 2
```

The source build fetches nlohmann/json `3.11.2` and builds bundled dependencies.
The executable uses build-directory rpaths; it is not a portable application
bundle.

## Run

The model directory must contain `model.onnx`, `model_eb.onnx`, `config.yaml`,
`am.mvn`, `tokens.json` and `seg_dict`. Input audio must be raw signed
little-endian 16-bit mono PCM at 16 kHz; WAV headers must be removed before
invoking the probe.

```bash
target/seaco-validation-np3tss9o/probe-build/seaco_native_probe \
  target/seaco-validation-np3tss9o/models/onnx-fp32 \
  target/seaco-validation-np3tss9o/fixtures/rare-terms.pcm 熵舟 声澈 弈澜 砺言
```

One model instance performs baseline / dictionary / baseline inference on the
same PCM. A nonzero exit status means initialization/inference failed or the two
baseline transcripts differed. Equality is an isolation check, not proof of
hotword accuracy. The report includes embedding count, load/embedding/inference
times, and process peak resident memory (cumulative, not incremental
allocation).

The upstream compiler splits inline hotwords on spaces, truncates each to ten
tokens, and may discard out-of-vocabulary terms. This probe rejects whitespace
in individual terms rather than pretending to support English phrases. It
includes the upstream blank embedding in the reported count.

See [validation evidence](../../../../docs/research/seaco-native-validation.md)
for hashes, observed outputs, limitations, and the current integration decision.
