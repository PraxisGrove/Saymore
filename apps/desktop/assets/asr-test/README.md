# ASR self-test fixtures

`standard-zh.pcm` is the existing bundled Chinese recognition fixture (16 kHz,
mono signed little-endian 16-bit PCM), used by Chinese-capable local models.

`standard-en.pcm` is disposable synthetic speech generated on macOS with the
Samantha voice at rate 150, then converted with `afconvert` to the same PCM
format. It says: “The quick brown fox jumps over the lazy dog.”

- 43,792 samples, 2.737 seconds, 87,584 bytes.
- SHA256: `b68e7b4e1c193648cc33045b1f669489253a0045b5e21c4e87127b79e0a9e071`.
- Added for Moonshine Tiny's English-only activation and recognition self-test.
- Contains no user recording or private transcript.

These fixtures check inference availability. They do not establish general ASR
accuracy or suitability for accents, mixed languages, background noise or names.
