# ASR Modes And Dictionary Support

## Meaning Of The UI Label

Whisper and Qwen offer **Non-streaming** and **Streaming** in a segmented
control. SeACo displays **Non-streaming** and Zipformer displays **Streaming**
as read-only text. Zipformer processes audio during capture and delivers the
transcript at the end of the recording. The control exposes the mode explanation
on hover and to assistive technology. The desktop warns after nine minutes and
stops at ten minutes for all four models. The adapter limits below also apply to
integrations that feed audio directly into those adapters.

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

| Model                  | Mode currently used by Saymore             | Capability qualification                                                                                                                                                                                                                                                      |
| ---------------------- | ------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Qwen3-ASR 1.7B         | Selectable streaming / background segments | Both modes use Q8_0 GGUF and the resident speech.cpp worker (Metal on Apple silicon). Streaming ports the official cumulative-audio and token-rollback algorithm; non-streaming retains complete overlapping windows. This is not the official Python/vLLM runtime.           |
| Whisper large-v3-turbo | Selectable streaming / background segments | Streaming repeatedly decodes the current window for replaceable previews, then confirms complete 29-second windows and the final tail with full context. Both modes use the same Q8_0 model and whisper.cpp. This is not a native incremental acoustic encoder.               |
| SeACo-Paraformer       | Background segments                        | The pinned SeACo graph consumes complete segment features and vocabulary embeddings. Its adapter submits complete windows to a background worker during capture. Do not infer live-audio support from the separate streaming Paraformer model that was previously integrated. |
| Zipformer              | While recording                            | The pinned artifact is a streaming transducer, and `OnlineRecognizer` drains ready frames during audio pushes and emits interim hypotheses. It also returns a final result. That does not establish an interchangeable offline graph for these exact files.                   |

All four supported adapters use the same `StreamingSpeechRecognizer` /
`StreamingRecognitionSession` ports for ordered audio chunks. Those historical
names do not declare model capability. The port documentation explicitly allows
buffered finalization. Qwen and Whisper in non-streaming mode, and SeACo, share
a background worker: complete windows are submitted during capture and the
remainder at `finish`. Non-streaming Qwen uses 30-second windows for all
vocabulary settings, with one second of overlap; Whisper retains 29 seconds;
SeACo retains 30 seconds. Completed text is collected on subsequent audio
pushes. This is coarse segment recognition, not token-level live audio
streaming. Short utterances still decode at finish. Each background session is
limited to 30 minutes of input, bounding queued audio. Cancellation discards
queued work and suppresses callbacks; native inference already running cannot be
interrupted and can temporarily retain the decoder.

Qwen and Whisper each have an independent persisted **Streaming /
Non-streaming** selector. Non-streaming remains the default. Changing it
preserves the selected provider and applies to the next recording; the active
recording keeps its mode and vocabulary snapshot. SeACo and Zipformer retain
their existing paths.

Qwen streaming accumulates audio in two-second steps. The first two decodes are
unfixed; subsequent calls roll back five native tokenizer tokens and force the
retained assistant prefix before generating a suffix. Finish flushes a shorter
tail; an exact chunk boundary does not trigger a second pass. The recording
overlay shows the replaceable hypothesis on one line inside a 360 × 35 logical
pixel recording capsule. Cancel and finish stay fixed on either side. Text
smoothly follows the latest tail as it grows, with a fading left edge when it
overflows; it does not scroll continuously while speech pauses. Whitespace is
normalized for the single line. Before the first hypothesis, the capsule shows
the audio waveform. After stop, it retains the draft beside the processing
indicator. Non-streaming keeps the compact 120 × 35 capsule. Callbacks are
checked against the recording's session identity on the UI thread. Only the
final result enters the existing polishing and delivery workflow.

Streaming is bounded to 20 minutes of total audio. Capture does not wait for
inference. When inference falls behind, queued steps are combined into the
latest cumulative audio before the next decode: intermediate previews may be
skipped, but audio is preserved. This coalescing is a port scheduling policy;
model limits and truncation still fail recognition explicitly. Cumulative audio
is reprocessed, so compute grows with duration. This is not an incremental
acoustic encoder, and early previews do not guarantee a shorter wait after stop.
See
[the official-algorithm port record](../research/qwen-official-streaming.md).
The earlier [mode assessment](../research/recognition-mode-switching.md) remains
background research; its generic rolling-preview proposal was not implemented.

Whisper streaming uses two-second input steps and repeatedly decodes the growing
current window. Preview calls can shorten the acoustic context; each full
29-second window and the final tail are confirmed with the original full-context
decoder. Finished windows are released, bounding per-call audio and compute as
the recording grows. The worker merges pending preview work while retaining all
audio and rejects input over 30 minutes. A preview for unchanged audio does not
replace full-context finalization. The shared floating overlay shows provisional
text; only the confirmed final transcript enters polishing/delivery.

With the current large-v3-turbo model, very short utterances may finish before
the first preview. Streaming adds repeated work and can increase stop-to-final
latency. See [Whisper streaming](../research/whisper-streaming.md) for sources,
implementation differences, measured latency and recognition quality.

## Hotwords And Personal Dictionary

Each introduction describes how the integrated model uses the personal
dictionary for names and terms. Users manage those entries in Dictionary. The
recognition paths and limits for the four integrated models follow.

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

Qwen and Whisper default to non-streaming. Default mode selection should use a
path that is both supported by the pinned artifact/runtime and validated for the
user's device. For low delay after the recording stops, doing inference during
capture is a useful target; Zipformer already does this. It does not establish
that every streaming path is faster or more accurate than its final-only
counterpart. Repeated sliding-window decoding can increase total compute.

Before enabling another mode, compare first-interim latency, stop-to-final
latency, recognition accuracy, CPU/GPU cost, memory, and vocabulary behavior on
the same audio. SeACo needs validation of an appropriate incremental path. Do
not change the user's selected model or advertise “fastest” on the basis of a
model name alone. MOSS is retired from the desktop selection.

## Evidence

- [Qwen3-ASR official README](https://github.com/QwenLM/Qwen3-ASR#streaming-inference)
  explicitly describes streaming/offline unified inference and the streaming
  backend requirement.
- [whisper.cpp 1.8.3 streaming example](https://github.com/ggml-org/whisper.cpp/blob/v1.8.3/examples/stream/README.md)
  describes periodic microphone transcription and a sliding-window mode.
- Current adapters: `qwen3_asr.rs`, `whisper_asr.rs`, `seaco_asr.rs` and
  `zipformer_asr.rs` under `crates/infra/src`.

Current scheduling follows the pinned artifacts, windows and vocabulary rules
above. The older [device latency results](../research/asr-background-latency.md)
measure background scheduling; current Qwen artifacts and acceptance are
recorded in [GGUF integration](../research/qwen3-gguf-integration.md). Current
streaming implementation and comparison results are recorded in the
[official-algorithm port](../research/qwen-official-streaming.md).
