# Standard recognition test audio

`standard-zh.pcm` is the PCM payload from AISHELL-1 utterance
`BAC009S0006W0280`. It contains 61,744 signed little-endian 16-bit samples at 16
kHz mono (3.859 seconds). The reference transcript is:

> 可以通过语音进行人机交互

Source dataset: <https://openslr.org/33/>

Mirror used to retrieve the source WAV:
<https://huggingface.co/datasets/AISHELL/AISHELL-1>

AISHELL-1 is distributed under the Apache License 2.0. That license applies only
to the AISHELL-1 sample, not to Saymore itself; Saymore's license remains the
repository-root `LICENSE`. A copy of the dataset license is included as
`AISHELL-1-LICENSE-Apache-2.0.txt`. Saymore sends this bundled sample only when
the user explicitly runs a provider recognition test. The provider transcript is
displayed transiently and is not persisted.
