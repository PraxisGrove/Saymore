# Local Voice Evaluation Recorder

This local-only workbench records and evaluates the reusable Saymore voice
benchmark. It binds to `127.0.0.1`, records 16 kHz mono PCM WAV, and writes
accepted clips under `recordings/<case-id>/`. Recording files, metadata, and run
artifacts are ignored by Git.

## Start

```bash
node tests/voice-evaluation/server.mjs
```

Open `http://127.0.0.1:4173`, allow microphone access, and record the first
incomplete case. Listen to each clip before selecting `保存并下一条`.

Each accepted case has this layout:

```text
recordings/R01/
  recording.wav
  metadata.json
```

`cases.json` is the canonical manifest for the recorder and the future batch
evaluation runner. In `批量评测`, select recorded cases, one ASR provider, and
one or more LLM providers. The workbench requires explicit remote-send
confirmation before starting the Rust runner. It submits each WAV to ASR once,
then submits the resulting transcript to the selected LLM providers in parallel.

The runner opens source WAVs read-only and writes each run separately:

```text
runs/<run-id>/
  request.json
  status.json
  progress.json
  result.json
  runner.log
```

The browser never receives provider API keys. It reads safe provider metadata
from `saymore-eval providers`; the Rust runner loads credentials from the current
Saymore Dev provider catalog and reuses the production ASR and LLM adapters.
