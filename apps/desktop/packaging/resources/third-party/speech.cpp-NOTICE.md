# Native Qwen inference runtime

Saymore packages speech.cpp 0.8.11 as `saymore-speech`, built from
`nyosegawa/speech.cpp@c4bdcd0e751bd29460cb1353e6d54499828a1d90` and its ggml
submodule `nyosegawa/ggml@cbdeccfd49ec9c702c175c273d36c47520cf4710`. Saymore
applies `crates/infra/native/qwen-streaming.patch` to add assistant-prefix
continuation, tokenizer rollback, a generation budget and raw transcript output.
The streaming algorithm follows QwenLM/Qwen3-ASR revision
`7c6daf77a2421100f5fb066495372c00129d39ff` (Apache-2.0); its Python/vLLM runtime
is not distributed with Saymore.

The adjacent license files cover speech.cpp and ggml (MIT), cpp-httplib (MIT),
and miniaudio (its upstream public-domain/MIT-0 terms). These native components
retain their own licenses; Saymore's application license is unchanged.

The Qwen3-ASR 1.7B model is downloaded separately from
`sakasegawa/Qwen3-ASR-1.7B-GGUF@81e2423d24073cdeeccb5951df16accc89e075a2`. The
checkpoint is Qwen's Apache-2.0 model, converted to Q8_0 GGUF. Its source is
<https://huggingface.co/Qwen/Qwen3-ASR-1.7B>.

No Arion application source is included in Saymore.
