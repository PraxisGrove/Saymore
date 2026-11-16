# ASR Modes And Dictionary Support

## Meaning Of The UI Label

The local-model details page describes **Saymore's current recognition mode**:

- `Current mode: recognize while recording` / `当前模式：边录音边识别`.
- `Current mode: background recognition in segments` / `当前模式：后台分段识别`.

This is an integration status, not a permanent streaming/non-streaming
classification of the model. Do not translate “final only” as a claim that the
model cannot support streaming. The mode appears on its own text row instead of
being grouped with the model's language and local-processing badges.

Keep four concepts separate:

1. **Model and artifact capabilities.** A model may support multiple modes, or a
   model family may publish different streaming and offline artifacts.
2. **Current runtime integration.** The selected graph, inference API and host
   state machine determine whether Saymore decodes during capture or at finish.
3. **Presentation and delivery.** Suppressing interim text does not convert a
   streaming engine into an offline engine. Interim hypotheses and final text
   delivery are different contracts.
4. **Network use.** “Offline” in a local-processing badge means no recognition
   network connection is required after installation. It is independent of
   whether recognition is incremental.

## MOSS Removal

MOSS has been removed from the application and benchmark tools after continuous
speech testing exposed unacceptable generation backlog. Its adapter, model
manifest, selection/storage/history branches, dedicated tests and exclusive
dependencies have been deleted. No MOSS-specific compatibility or migration code
is retained. Historical measurements remain in research documents and git
history. See [latency follow-up](../research/asr-latency-follow-up.md).

## Current Local Integrations

| Model                  | Mode currently used by Saymore | Capability qualification                                                                                                                                                                                                                                                                                                                   |
| ---------------------- | ------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Qwen3-ASR 1.7B         | Background segments            | Qwen explicitly documents unified streaming/offline inference with one model. Saymore uses Q8_0 GGUF and a resident speech.cpp worker on complete windows during capture; Apple silicon uses Metal. This is bounded offline inference, not Qwen's native streaming interface.                                                              |
| Whisper large-v3-turbo | Background segments            | whisper.cpp includes a microphone streaming example using repeated windowed transcription. Saymore currently invokes `full` on complete windows in a background worker during capture. Repeated sliding-window transcription is not implemented here; it is not evidence that Whisper has native incremental decoder state for live audio. |
| SeACo-Paraformer       | Background segments            | The pinned SeACo graph consumes complete segment features and vocabulary embeddings. Its adapter submits complete windows to a background worker during capture. Do not infer live-audio support from the separate streaming Paraformer model that was previously integrated.                                                              |
| Zipformer              | While recording                | The pinned artifact is a streaming transducer, and `OnlineRecognizer` drains ready frames during audio pushes and emits interim hypotheses. It also returns a final result. That does not establish an interchangeable offline graph for these exact files.                                                                                |

All four supported adapters use the same `StreamingSpeechRecognizer` /
`StreamingRecognitionSession` ports for ordered audio chunks. Those historical
names do not declare model capability. The port documentation explicitly allows
buffered finalization. The three supported offline-model adapters share a
background worker: complete windows are submitted during capture and the
remainder at `finish`. Qwen3 uses 30-second windows for all vocabulary settings,
with one second of overlap; Whisper retains 29 seconds; SeACo retains 30
seconds. Completed text is collected on subsequent audio pushes. This is coarse
segment recognition, not token-level live audio streaming. Short utterances
still decode at finish. Each session is limited to 30 minutes of input, bounding
queued audio. Cancellation discards queued work and suppresses callbacks; native
inference already running cannot be interrupted and can temporarily retain the
decoder.

There is currently no selectable recognition-mode setting. Adding one requires
implementing and validating the corresponding runtime path for each supported
artifact, including interim revision handling, audio boundaries, cancellation,
finalization, latency, and vocabulary behavior. Do not add a toggle that only
changes the label or hides partial results while claiming to switch inference
mode.

## Hotwords And Personal Dictionary

Local-model details also show whether the **current recognition integration**
uses the personal dictionary. A checked Hotwords badge and a detail section
identify supported integrations and distinguish recognition bias from prompt
guidance. Unknown or future model kinds default to “not integrated”; they must
not inherit a supported label merely because another model supports hints.

| Model            | Current dictionary path                          | Adapter limits                                                            |
| ---------------- | ------------------------------------------------ | ------------------------------------------------------------------------- |
| SeACo-Paraformer | Vocabulary embeddings participate in recognition | Up to 64 known terms, at most 10 vocabulary tokens per term               |
| Zipformer        | Transducer hotword bias during decoding          | Up to 64 supported terms, 64 characters per term, 1,024 UTF-8 bytes total |
| Qwen3-ASR        | Vocabulary text in the recognizer's prompt       | 128 UTF-8 bytes total                                                     |
| Whisper          | Initial-prompt tokens supplied to whisper.cpp    | 192 UTF-8 bytes before tokenization                                       |

The desktop snapshots the personal dictionary at the start of each dictation,
independently of whether text polishing is enabled. Each adapter filters and
bounds that snapshot. “Supported” describes a connected recognition path, not
guaranteed spelling, a measured improvement on every word, or an assertion that
all dictionary entries are sent. Prompt guidance and native bias are different
mechanisms, not an accuracy ranking. Optional text polishing is a separate use
of the dictionary after recognition.

## Choosing A Faster Mode

The current-mode label is read-only. Default mode selection should use a path
that is both supported by the pinned artifact/runtime and validated for the
user's device. For low delay after the recording stops, doing inference during
capture is a useful target; Zipformer already does this. It does not establish
that every streaming path is faster or more accurate than its final-only
counterpart. Repeated sliding-window decoding can increase total compute.

Before enabling another mode, compare first-interim latency, stop-to-final
latency, recognition accuracy, CPU/GPU cost, memory, and vocabulary behavior on
the same audio. Qwen3 and Whisper need additional runtime integration for their
alternative paths; SeACo needs validation of an appropriate incremental path. Do
not change the user's selected model or advertise “fastest” on the basis of a
model name alone. MOSS is retired from the desktop selection.

## Evidence

- [Qwen3-ASR official README](https://github.com/QwenLM/Qwen3-ASR#streaming-inference)
  explicitly describes streaming/offline unified inference and the streaming
  backend requirement.
- [whisper.cpp 1.8.3 streaming example](https://github.com/ggml-org/whisper.cpp/blob/v1.8.3/examples/stream/README.md)
  describes periodic microphone transcription and a sliding-window mode.
- Current adapters: `qwen3_asr.rs`, `whisper_asr.rs`, `seaco_asr.rs`,
  `zipformer_asr.rs`, and `moss_asr.rs` under `crates/infra/src`.
- The [MOSS integration record](../research/moss-native-integration.md) fixes
  the graph contract and distinguishes its final-recognition path from upstream
  long-context capabilities.

This implementation changes recognition scheduling as well as presentation. The
model files, per-model window boundaries, vocabulary snapshots and final text
joining rules remain unchanged. See the
[device latency results](../research/asr-background-latency.md) for measured
benefits and remaining platform/model validation.
