# macOS Apple Speech capability probe

Tested on 2026-07-26 with the signed `Saymore Preview.app` on:

- macOS 26.3.2
- Apple M5 (`aarch64`)
- system speech locale `zh_CN`
- Apple `Speech.framework` through `objc2-speech` 0.3.2

This is evidence for one machine and OS version. It is not evidence for Intel
Macs or macOS 12 through 15.

## Product conclusion

Apple Speech is suitable for the first macOS system-ASR adapter on the tested
machine:

- Saymore does not need an API key, billing account, or paid provider
  configuration.
- Saymore does not download or manage a model. Apple may manage language assets
  as part of macOS; the app must not describe those system assets as a Saymore
  model download.
- System-managed recognition may use Apple's network service. Apple documents
  that the service is freely available but may apply per-device limits and
  global app throttling. It is therefore free to integrate, not an unlimited
  service-level guarantee.

Sources:

- [SFSpeechRecognizer](https://developer.apple.com/documentation/speech/sfspeechrecognizer)
- [requiresOnDeviceRecognition](https://developer.apple.com/documentation/speech/sfspeechrecognitionrequest/requiresondevicerecognition)
- [NSSpeechRecognitionUsageDescription](https://developer.apple.com/documentation/bundleresources/information-property-list/nsspeechrecognitionusagedescription)

## Observed results

| Probe                                                 | Result                                                                           |
| ----------------------------------------------------- | -------------------------------------------------------------------------------- |
| Authorization                                         | First run changed from `not-determined` to `authorized` through the macOS prompt |
| System default locale                                 | Created as `zh_CN`; available; on-device supported                               |
| `zh-CN`                                               | Created as `zh-CN`; available; on-device supported                               |
| `en-US`                                               | Created as `en-US`; available; on-device not supported on this machine           |
| 3.096 s, production system-managed adapter            | Success; 14 partial results; final in 322 ms after rapid submission              |
| Second consecutive production adapter session         | Success; 14 partial results; same final text; 85 ms                              |
| 3.096 s, forced on-device                             | Success; 14 partial results; final in 87 ms after rapid submission               |
| Cancel after full submission                          | Task reported canceled; callback arrived; no final transcript was delivered      |
| 65 s, production adapter with real-time 100 ms chunks | All 65 s accepted; 297 partial results; final transcript; 65,931 ms total        |

Both production adapter sessions and the forced on-device probe produced the
same final text for the bundled non-sensitive Chinese test fixture. The second
session demonstrates that a completed task does not leak transcript or lifecycle
state into the next task. The 65-second result shows that this OS version did
not enforce the historically documented one-minute stop for this request. The
adapter still handles early completion and system errors because Apple can vary
behavior by OS, locale, and service availability.

## Production integration

`MacOsSpeechRecognizer` implements the existing provider-independent streaming
port in `crates/infra`. It owns Apple objects on a dedicated worker, accepts
Saymore's existing 16 kHz mono `i16` chunks, marks production requests as
dictation, applies up to 100 dictionary hints as contextual strings, and maps
authorization, availability, timeout, Apple callback errors, and invalid final
results into `SpeechRecognitionError`. The task hint guides recognition but does
not guarantee a particular inverse text normalization result.

The Models page exposes this adapter as macOS Dictation. Selection is persisted
as the active ASR Provider without deleting saved Volcengine or
OpenAI-compatible configuration. The card reflects Speech Recognition
authorization and recognizer availability and opens provider details. The detail
switch selects an authorized, available recognizer; its permission action
requests authorization on first use and opens the corresponding System Settings
privacy pane after denial or restriction. Both Preview and release runtimes
resolve the same persisted Provider catalog before starting a dictation session.

## Scope not exercised

- Denial, restriction, and post-authorization revocation were not forced because
  doing so would mutate the user's existing macOS privacy settings. The product
  recovery path is implemented, but still needs a manual acceptance pass before
  release.
- Offline system-managed recognition was not tested. It is not a product
  requirement.
- Intel and older supported macOS releases remain a device-matrix gap.
- No English speech fixture was available, so `en-US` capability creation was
  checked but English transcription accuracy was not.
- The automated UI driver cannot emit the configured modifier-only global
  shortcut. A final manual pass must press the Preview shortcut, speak into the
  selected physical microphone, press the shortcut again, and confirm delivery
  into an editable target.

## Acceptance implication

System-managed recognition is now implemented behind the existing streaming port
and validated in the signed Preview probe. The macOS card can proceed to
provider selection and permission-state wiring. It should say that no account or
model download is required, must not promise unlimited availability, and should
expose a recoverable state for Apple throttling or temporary service
unavailability.
