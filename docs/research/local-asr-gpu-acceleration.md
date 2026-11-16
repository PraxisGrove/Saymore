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

| Model                       | macOS                                | Windows | Constraint                                                                                                                                                  |
| --------------------------- | ------------------------------------ | ------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Whisper large-v3-turbo Q8_0 | Requests Metal automatically         | CPU     | macOS target enables whisper-rs `metal`; no weight conversion required.                                                                                     |
| Qwen3-ASR                   | Metal on Apple silicon; CPU on Intel | CPU     | Pinned speech.cpp 0.8.11 worker and Q8_0 GGUF; selected backend failures are explicit, with no ONNX fallback. See [integration](qwen3-gguf-integration.md). |
| Zipformer                   | CPU                                  | CPU     | Its sherpa-onnx build uses the CPU ONNX Runtime.                                                                                                            |
| SeACo-Paraformer            | CPU                                  | CPU     | Uses the existing linked CPU ONNX Runtime through ORT's alternative backend; must retain one runtime when adding acceleration.                              |

macOS system dictation is managed by Apple Speech. Saymore does not select its
compute device or promise GPU execution.

Whisper's macOS dependency embeds the Metal shader source in its native library.
No separate shader resource, model download format, or GPU runtime installer is
introduced. `use_gpu` is enabled only on macOS. The pinned whisper.cpp keeps CPU
available when it cannot find a GPU device; this is not a guarantee of recovery
from every Metal allocation, initialization, or inference failure. CPU threads
are still used for host work, and GPU acceleration does not mean zero CPU use.

## Verification And Open Work

The [Qwen GGUF integration](qwen3-gguf-integration.md) records its current
native Metal worker, verified installation, real adapter replays, and macOS
packaging acceptance. It replaces Qwen's former CPU ONNX path.

The [remaining-model investigation](other-asr-native-acceleration.md) confirms
that Whisper already uses native Q8_0 inference with Metal. Enabling Flash
Attention on Apple M4 reduced the short-fixture median from 2.565 s to 2.050 s,
but repeatedly dropped two extra characters from continuous Chinese. That
candidate was reverted; the production adapter retains its existing setting.
This comparison measures two Metal configurations, not CPU versus GPU.

The actual linked ONNX Runtime reports CoreML unavailable and CPU available.
SeACo and Zipformer retain their CPU implementations. Their installed quantized
graphs contain dynamic integer matrix multiplication unsupported by the
inspected CoreML operator registry. Replacing the runtime or export requires
actual graph partitioning and quality/latency measurements; a provider string
alone cannot establish accelerator execution. Neither architecture is supported
by the pinned speech.cpp runtime.

Further work: broader human/noisy/mixed-language acceptance for Whisper native
optimizations; compatible graph/runtime experiments for SeACo or Zipformer only
when measured latency, recording-time CPU or power justify them. Windows/Intel
inference and minimum-OS package acceptance remain platform-specific work.
