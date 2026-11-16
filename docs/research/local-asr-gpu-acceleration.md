# Local ASR Acceleration

## Selection Policy

Prefer automatic selection of an accelerator supported by both the installed
model and its inference runtime. The presence of a GPU alone is insufficient:
Metal, CUDA, DirectML, and CoreML are different runtime integrations, and graph
operators and quantization formats must also be supported.

The current implementation has no user-selectable compute setting. A future
model-level setting should distinguish Auto from CPU only. If GPU only is
introduced, it must fail explicitly when unavailable rather than silently use
CPU. The UI should report the backend actually selected, separately from the
requested preference. Do not label a model GPU-ready solely because the build
contains an accelerator backend.

## Current Implementation

| Model                        | macOS                                | Windows | Constraint                                                                                                                                                  |
| ---------------------------- | ------------------------------------ | ------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Whisper large-v3-turbo Q8_0  | Requests Metal automatically         | CPU     | macOS target enables whisper-rs `metal`; no weight conversion required.                                                                                     |
| Qwen3-ASR                    | Metal on Apple silicon; CPU on Intel | CPU     | Pinned speech.cpp 0.8.11 worker and Q8_0 GGUF; selected backend failures are explicit, with no ONNX fallback. See [integration](qwen3-gguf-integration.md). |
| Zipformer                    | CPU                                  | CPU     | Its sherpa-onnx build uses the CPU ONNX Runtime.                                                                                                            |
| MOSS-Transcribe-Diarize FP16 | CPU                                  | CPU     | Experimental; shares the linked CPU ONNX Runtime with SeACo. See the [integration record](moss-native-integration.md).                                      |
| SeACo-Paraformer             | CPU                                  | CPU     | Uses the existing linked CPU ONNX Runtime through ORT's alternative backend; must retain one runtime when adding acceleration.                              |

macOS system dictation is managed by Apple Speech. Saymore does not select its
compute device or promise GPU execution.

Whisper's macOS dependency embeds the Metal shader source in its native library.
No separate shader resource, model download format, or GPU runtime installer is
introduced. `use_gpu` is enabled only on macOS. The pinned whisper.cpp keeps CPU
available when it cannot find a GPU device; this is not a guarantee of recovery
from every Metal allocation, initialization, or inference failure. CPU threads
are still used for host work, and GPU acceleration does not mean zero CPU use.

## Verification And Open Work

On the development Apple M4:

- Desktop all-target compilation succeeds with `--locked`.
- All eight existing Whisper unit tests pass with Metal linked. They exercise
  audio segmentation, cancellation, empty audio, vocabulary propagation and
  isolation, and artifact rejection; they do not run real GPU inference.
- Cargo's macOS feature tree includes `whisper-rs/metal` and `_gpu`; native
  CMake output confirms `GGML_METAL=ON` and `GGML_METAL_EMBED_LIBRARY=ON`.
- A real-audio CPU baseline was attempted using the production probe and the
  bundled public Chinese fixture in `target/asr-gpu-review`. No installed model
  was available. Downloading the pinned Hugging Face artifact failed; a separate
  curl check reported connection reset. No performance or GPU-execution claim
  can be made from this attempt.
- Windows feature-tree inspection in offline mode was blocked by an uncached
  `winapi` dependency. Windows compilation and packaged execution remain
  pending.

Next: obtain the pinned verified artifact, run repeated production-adapter
sessions with and without Metal, confirm the actual compute backend, and compare
latency and output on Chinese, mixed-language, silence, and long audio. Validate
packaged macOS execution and no-GPU fallback before treating acceleration as
release-accepted. Then evaluate ONNX accelerator runtime packaging and operator
coverage for the remaining CPU models; adding only a UI switch cannot enable
them.
