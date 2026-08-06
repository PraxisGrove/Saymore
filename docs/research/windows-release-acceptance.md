# Windows release acceptance

Last updated: 2026-08-06

This document records durable Windows x64 acceptance evidence for the four
pinned local ASR models and the remaining checks required before a public
release. Local build paths, hashes for unpublished candidates, and
workstation-specific installation details are intentionally omitted.

## Automated and runtime evidence

All four local models completed the production Windows runtime loop:

- verified download, SHA-256 corruption detection, and atomic repair;
- pause, HTTP Range resume across installer instances, and reinstall;
- production adapter load and two consecutive real-audio sessions;
- Chinese, Chinese-English mixed speech, empty audio, cancellation, and an
  82.2-second recording;
- installed-directory size and process working-set measurement.

Activation remains explicit. Downloading a model does not select it. Selecting
one loads the adapter and transcribes the bundled non-sensitive self-test audio
before the Provider catalog changes. The active model cannot be deleted until
the user switches to another Provider.

| Model                  | Manifest bytes |   Installed bytes |        Load | Chinese inference | Working-set delta |
| ---------------------- | -------------: | ----------------: | ----------: | ----------------: | ----------------: |
| Paraformer             |    237,202,501 |   237,203,168-169 | 0.96-1.17 s |       0.28-0.38 s |        290-296 MB |
| Whisper large-v3-turbo |  1,036,613,791 | 1,036,614,484-485 | 1.33-1.95 s |       1.61-1.75 s |    1.089-1.095 GB |
| Qwen3-ASR 1.7B INT8    |  2,404,222,421 | 2,404,223,686-688 | 4.67-7.04 s |       1.68-2.10 s |    2.504-2.506 GB |
| SenseVoiceSmall        |    240,193,660 |       240,194,499 | 1.22-1.52 s |       0.12-0.16 s |        310-314 MB |

Installed size includes Saymore's verification marker. Working-set figures are
process deltas observed around cold adapter load, not model-only allocations.
Mixed-language quality remains model-dependent; stable execution is not a claim
of exact English product-name recognition.

The shared lifecycle and failure tests cover live-session FIFO queues, bounded
download concurrency, network recovery, insufficient disk space, hash mismatch,
same-size corruption, long Unicode paths, failed activation, failed settings
persistence, and active-model deletion protection. Restored downloads remain
outside the live queue and paused until the user explicitly continues one.

Windows integration checks also established that:

- the executable is x64, per-monitor DPI aware, and long-path aware;
- sherpa-onnx, ONNX Runtime, and the MSVC CRT are statically linked;
- the NSIS installer uses current-user installation and preserves application
  data during upgrade and uninstall;
- the portable executable uses the same application-owned data paths and its
  removal also preserves user data;
- UTF-16 history copy and production text delivery into Windows Notepad preserve
  ASCII and Chinese text exactly;
- the Right Alt monitor emits one toggle while preserving foreground focus;
- Microsoft Defender reported no detections for the tested candidates;
- the workspace checks, documentation tests, desktop navigation test, release
  build, dependency audit, and repository size gate passed for the candidate.

## Defects found and fixed

1. The shared HTTP client now sends a stable `Saymore/<package version>` user
   agent so ModelScope does not reject Qwen3-ASR downloads.
2. The Qwen3-ASR adapter strips the known `<asr_text>` control prefix before
   overlap deduplication at long-recording segment boundaries.
3. Whisper uses segments shorter than sherpa-onnx's 30-second input limit.
4. Native `Edit` and `RichEdit*` controls receive synchronous paste dispatch;
   custom controls retain the guarded keyboard fallback.
5. Right Alt press and release are consumed as one dictation gesture without
   leaking an Alt action to the foreground application.
6. Persisted downloads restore as individually resumable paused tasks instead of
   disabled queued tasks; continuing one does not start the others.

## Remaining release matrix

The canonical clean-machine and packaged-application checklist lives in the
[Windows release candidate matrix](../testing.md#windows-release-candidate-matrix).
The remaining blockers are Authenticode signing and SmartScreen reputation,
clean-machine installation, microphone capture, Word and Chromium delivery,
multi-display DPI behavior, and upgrade and uninstall data retention.

Automation on a development workstation is evidence for deterministic behavior,
not a substitute for microphone, reputation, clean-machine, or third-party
application checks. Windows system speech and MSIX remain outside the current
beta scope.
