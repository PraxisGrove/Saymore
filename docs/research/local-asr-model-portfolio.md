# Saymore 本地 ASR 模型组合建议

调研日期：2026-07-15
范围：面向可下载的本地听写模型，比较 `Whisper large-v3-turbo`、`paraformer-zh-streaming` Q8、`Qwen3-ASR-1.7B` 的可行 8-bit 路线，以及 `SenseVoiceSmall`。除仓库现有实测外，外部事实只引用模型/运行时发布方的一手资料。

## 结论

**Whisper 没有整体“落后”，但对于 Saymore 的中文优先、本地、低延迟产品，它不应再是唯一默认模型。**

它依旧有三项难以替代的价值：成熟的多语言 ASR、语音翻译为英文，以及 MIT 许可。OpenAI 的 `turbo` 是 `large-v3` 的快速变体，约 809M 参数；完整 `large` 为 1.55B。[OpenAI Whisper README](https://github.com/openai/whisper#available-models-and-languages)

相对较新的中文向模型，Whisper 的弱点是产品维度而非失效：

- **不是原生实时流式。**官方 `transcribe()` 以滑动 30 秒窗口进行自回归解码，模型卡也明确称开箱不能实时转录；近实时体验要由运行时、VAD 和端点策略另行实现。[README](https://github.com/openai/whisper#python-usage) [模型卡](https://github.com/openai/whisper/blob/main/model-card.md#broader-implications)
- **语言/方言不是同等质量保证。**OpenAI 明确说明低资源语言及不同口音、方言的效果不均；这不适合把它当作粤语、台湾繁中或中国方言的质量承诺。[模型卡](https://github.com/openai/whisper/blob/main/model-card.md#performance-and-limitations)
- **资源效率不是其强项。**官方表把 `turbo` 标为约 6 GB VRAM、`large` 约 10 GB VRAM（均为 A100 英语测量的近似值，不可外推为桌面实测）。对照之下，非自回归的中文模型可更小、更适合 CPU/边缘端。[OpenAI README](https://github.com/openai/whisper#available-models-and-languages)

因此建议采用 **3 个稳定档 + 1 个实验档**，让用户按语言和设备下载，而非让所有用户下载一个“最大最准”模型。

| 档位与下载名 | 推荐职责 | 语言 / 口音边界 | 流式与本地运行时 | 下载与许可结论 |
| --- | --- | --- | --- | --- |
| **默认：Paraformer 中文实时 Q8** | 按键听写的默认档；优先低延迟、低内存。 | 官方模型表：中文、英文，220M。并不应承诺繁中、粤语或方言质量。 | 官方示例按 600 ms chunk 保持 cache 输入，属于原生分块流式。Saymore 已以 `sherpa-onnx` 验证 Q8。 | 本仓库 Q8 为 227.2 MiB、峰值 RSS 503 MiB、RTF 0.091（四 CPU 线程，FLEURS 中文）；详见[既有基准](./paraformer-onnx-benchmark.md)。FunASR 代码是 MIT，但模型权重适用其[模型许可](https://github.com/modelscope/FunASR/blob/main/MODEL_LICENSE)，要求保留来源和模型名。 |
| **轻量多语：SenseVoiceSmall Q8** | 粤语及普通话/英语/日语/韩语的轻量离线档；适合最终稿或 VAD 分段结果，不替代实时默认档。 | 已发布 Small checkpoint 明确覆盖普通话、粤语、英语、日语、韩语；没有台湾繁中或其他方言的发布方保证。 | 非自回归；官方提供 ONNX/Libtorch，并提供 CPU/edge GGUF 路线。其资料没有给出 Paraformer 式的 partial-result 流式接口，不能将 VAD 分段误称为原生流式。 | 234M 参数。官方发布物将 Q8 标为约 250 MB，须将它作为独立制品下载和实测。原始 checkpoint 模型卡标为 `other`/model-license，而官方 GGUF 制品卡标为 Apache-2.0；发布前必须按实际分发制品做许可复核。 [SenseVoice 官方仓库](https://github.com/FunAudioLLM/SenseVoice) [原始模型卡](https://huggingface.co/FunAudioLLM/SenseVoiceSmall) [GGUF 模型卡](https://huggingface.co/FunAudioLLM/SenseVoiceSmall-GGUF) |
| **国际化：Whisper large-v3-turbo** | 多语言、英文翻译和兼容性基线；不是“中文实时”默认档。 | 广泛多语言 ASR、语言识别和语音译为英文；对各方言质量不作同等保证。 | 原始官方实现是 Python/PyTorch + FFmpeg，按 30 秒自回归窗口处理。macOS 与 Windows 都有 FFmpeg 安装指引，但官方没有为 Apple Silicon/Windows CPU 给出此大模型的桌面实时性能承诺。 | 809M 参数，MIT（代码和权重）。`turbo` 不训练翻译任务，若需翻译必须改用其他多语言 Whisper checkpoint。 [官方 README](https://github.com/openai/whisper) |
| **实验高精度：Qwen3-ASR-1.7B 8-bit** | 复杂声学环境、繁体中文/粤语/中国方言、中英混说的准确率候选；先做受控试验再上架。 | 官方称 30 种语言、22 种中国方言，明确列出粤语、吴语、闽南语；其评测也含 `CV-zh-TW`。这使其最值得测试台湾繁中和方言，但不等于目标用户集已达标。 | 官方提供离线与流式；**流式目前仅 vLLM，且不支持 batch 或时间戳返回**。官方示例以 CUDA + BF16 / vLLM 为主，建议 FlashAttention 2；未给出 Apple Silicon 或 Windows CPU 的官方桌面运行矩阵。 | 权重 Apache-2.0。官方 `Qwen/Qwen3-ASR-1.7B` 文件只提供原始 safetensors，示例亦使用 BF16；**没有官方发布的 8-bit checkpoint**。8-bit 应由 Saymore 从固定官方 revision 自行转换，或明确列为第三方制品，并对每个设备回归评测。 [官方仓库](https://github.com/QwenLM/Qwen3-ASR) [官方模型卡](https://huggingface.co/Qwen/Qwen3-ASR-1.7B) |

### 为什么不是只保留 Whisper

在相同发布方的中文/粤语能力定位上，SenseVoiceSmall 明确覆盖粤语，Qwen3-ASR 明确覆盖 22 种中国方言；Paraformer 则提供真正的分块流式路径。Whisper 保留“泛语言、翻译、成熟基线”的位置即可。不能由不同厂商的宣传或不同运行时的 WER/CER 表，推导出任一模型在 Saymore 音频上绝对更准。

Qwen 的官方评测可说明它值得进入候选而非证明 Q8 胜出：比较是在 `torch.bfloat16`、vLLM、贪心解码下完成，并非 8-bit，也没有纳入 Paraformer。目标设备上的量化损失、端点策略和专名错误必须自行测量。[Qwen 评测设置](https://github.com/QwenLM/Qwen3-ASR#evaluation)

## 上架顺序与门槛

1. **先发 Paraformer Q8。**它已有 Saymore 同运行时的性能与质量基线，适合中文实时默认体验。
2. **再发 SenseVoiceSmall Q8。**明确标为“五语轻量最终转写”，用粤语、普通话、英语、日语、韩语的真实听写集验证；它不承担 partial-result 契约。
3. **提供 Whisper `large-v3-turbo`。**标为“国际化 / 翻译能力需要其他 Whisper 变体”，避免用户误以为它专长于中文方言。
4. **最后以“实验性”推出 Qwen 1.7B 8-bit。**没有官方 8-bit 发布物时，产品必须显示转换来源、原始模型 revision、量化方法、最低内存和目标平台；不满足这些条件就不要提供下载按钮。

每一档在标记为稳定前，都用**同一设备、相同 VAD/切分/端点、相同文本规范化**测：普通话、台湾繁中、粤语、目标方言、中英混说、专名/数字与噪声。至少记录 CER/WER、关键实体错误率、首字 P50/P95、结束语音到最终文本 P50/P95、RTF、冷启动、峰值内存和模型下载大小。Q8 与原始精度权重必须成对报告，不能借用 BF16/FP16 成绩。

## 平台原则

“可本地加载”不等于“适合桌面交付”。Paraformer 的既有数据只覆盖当前 Saymore 运行时；SenseVoice 的官方 GGUF/CPU 路线和 Whisper 的官方 Python 路线可以作为候选，但 macOS Apple Silicon、Windows CPU、NVIDIA 分别都要独立验收。Qwen 的官方快速流式路径是 CUDA/vLLM，故在 Apple Silicon 和 Windows CPU 上应先视为研发验证项，而不是承诺的产品功能。
