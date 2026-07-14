# Typeless ASR 归因与 Whisper、Paraformer、Qwen3-ASR 对比

调研日期：2026-07-15
范围：核查 Reading Outpost 对 Typeless 的介绍能否证明其 ASR 为 OpenAI Whisper，并为 Saymore 的本地听写候选作技术定位。除特别标明的本机制品检查外，只采用模型/产品发布方的一手资料。本文不把厂商自测或第三方量化卡当作跨模型结论。

## 结论先行

1. **“Typeless 用的是开源 Whisper”目前不能证实。**用户提供的 [Reading Outpost 文章](https://readingoutpost.com/typeless/)（页面更新于 2026-07-09）是体验评测：全文没有 `Whisper`、`OpenAI`、`ASR` 或模型名称，也没有链接到技术分析或可复现证据。它不能支持该说法。
2. Typeless 官方只确认“转录在云端处理”，以及使用第三方 **LLM** 供应商“例如 OpenAI”；这既没有指名 ASR 供应商，也不等于调用 OpenAI 开源 Whisper。[Typeless Data Controls](https://www.typeless.com/data-controls) 明确把两件事分开描述。因而也不能从“OpenAI”反推为 Whisper。
3. 对 Saymore 的中文本地实时听写，当前最合理的**默认候选是已验证的 `paraformer-zh-streaming` Q8**：模型小得多、天然分块流式，且仓库已有同一运行时的准确率、内存和实时率数据。若目标是复杂环境、繁体中文/粤语/方言和更多语言的最终准确率，则 **Qwen3-ASR-1.7B 的精确 8-bit 变体**是更强的实验/高级档候选，但不应在没有目标硬件和量化后复测前取代默认模型。
4. **Whisper 不是一个单一可比较对象。**`tiny` 到 `large-v3` / `turbo` 相差数十倍；没有 Typeless 的确切 checkpoint、运行时与解码参数，更不能将“它可能用 Whisper”的体验外推为 Saymore 该选哪一版 Whisper。

## Typeless 的证据核查

| 结论 | 证据 | 置信度 |
| --- | --- | --- |
| Typeless 在云端进行转录 | 官方 Data Controls 写明转录在云端进行，以获得准确度和低延迟。 | 已确认：官方公开资料 |
| Typeless 使用第三方 LLM，示例为 OpenAI | 同一页面的措辞是 “LLM providers (such as OpenAI)”。 | 已确认：官方公开资料 |
| Typeless 的 ASR 是 OpenAI Whisper | 上述页面没有 ASR Provider 或模型名；文章没有模型技术内容。 | **未证实** |
| Typeless 的 “Whisper mode” 等同 Whisper 模型 | 价格/引导页中的该词是“低声说话”的产品模式；本机 2.0.1 只读字符串检查中它也是本地化 UI 文案，未发现可归因于 OpenAI Whisper 的模型文件或标识。云端服务可以随时换模型，静态未发现也不能证明服务端未使用。 | 不成立为证明 |

官方的功能页还确认它会移除填充词、处理自我修正、改写语气，并在 100+ 语言间自动检测。这说明最终文本至少包含超出逐字 ASR 的处理，但不公开 ASR、后处理和 LLM 的调用顺序或模型。[Typeless 功能说明](https://www.typeless.com/help/quickstart/key-features)

要把结论提升为“使用 Whisper”，需要至少一种直接证据：Typeless 的模型/供应商披露、可审计的服务器请求或官方安装包中明确的 Whisper checkpoint/SDK 配置。当前材料均不具备。对云端产品，客户端二进制即使没有本地模型也不能排除任意服务端模型。

## 候选模型的可比事实

| 候选（必须固定精确变体） | 中文与多语言 | 流式/延迟事实 | 资源与部署含义 | 适合的产品位置 |
| --- | --- | --- | --- | --- |
| **OpenAI Whisper** | 多语言 ASR、语言识别和语音译为英文；官方说明不同语言/口音表现不均，训练数据以英语为主。 | 官方 `transcribe()` 按 30 秒窗口自回归解码；模型卡明确“不直接支持实时转录”。可由其他运行时包装为近实时，但不是原生流式合同。 | 选型范围为 39M 到 1.55B；`turbo` 约 798/809M。官方 VRAM 表仅是 A100 英语测试的近似值，不能替代桌面实测。 | 国际语言兼容性、翻译和横向基准；不是在“Whisper（未指版本）”层级可判定的默认中文实时方案。 |
| **FunASR Paraformer-zh-streaming** | 官方模型表列为中文/英文、220M；不是通用 100+ 语种模型。 | 官方示例持续喂入 600 ms chunk，也提供 480 ms 配置；这是分块大小，**不是**完整端到端首字延迟承诺。Paraformer 论文采用非自回归并行解码，论文报告相对其 AR Transformer 实验超过 10x 加速。 | 有成熟的流式 API；本仓库固定 `paraformer-zh-streaming` + sherpa-onnx 的 Q8 已实测约 227 MiB 文件、503 MiB 峰值 RSS、RTF 0.091（四 CPU 线程、FLEURS 中文）。 | 中文优先、离线、常驻、低延迟的默认档。需要把 VAD、标点和热词/词典策略作为独立产品能力验证。 |
| **Qwen3-ASR-1.7B** | 官方称支持 30 种语言和 22 种中国方言，包含普通话、粤语、吴语、闽南语及繁体中文评测集。 | 官方支持离线和流式；开源本地流式当前要求 vLLM，且流式模式不支持 batch 和时间戳返回。未发布适用于单台桌面 1.7B 的首字/最终延迟合同。 | 官方参考实现使用 BF16 + CUDA/vLLM，并建议 FlashAttention 2。**官方发布页没有 8-bit checkpoint。**常见的 `mlx-community/Qwen3-ASR-1.7B-8bit` / 其他 MLX 8-bit 均为第三方转换，须分别锁定作者、提交/哈希、运行时和 Apple Silicon 机型；例如另一公开转换明确标注为第三方 MLX 转换、约 2.46 GB。 | 准确率、多语种、繁体中文和方言优先的高级本地档或 GPU 服务档；先作为实验候选。 |

Whisper 的模型能力、大小和 30 秒窗口来自 [OpenAI 官方仓库](https://github.com/openai/whisper) 与 [官方模型卡](https://github.com/openai/whisper/blob/main/model-card.md)。Paraformer 模型范围、220M 和流式 chunk 示例来自 [FunASR 官方仓库](https://github.com/modelscope/FunASR)；其非自回归架构和加速实验来自作者的 [Paraformer 论文](https://arxiv.org/abs/2206.08317)。Qwen3-ASR 的语言、方言、流式限制和官方部署路径来自 [Qwen 官方仓库](https://github.com/QwenLM/Qwen3-ASR) 与 [官方模型卡](https://huggingface.co/Qwen/Qwen3-ASR-1.7B)。第三方 8-bit 的例子见 [MLX 转换模型卡](https://huggingface.co/aufklarer/Qwen3-ASR-1.7B-MLX-8bit)，它本身不能证明官方 Qwen 的量化质量或性能。

## 准确率：能说什么，不能说什么

Qwen 的官方仓库发布了同一评测配置下的 **BF16/vLLM** 结果，比较了 `Qwen3-ASR-1.7B` 与 `Whisper-large-v3`，而没有比较 Paraformer，也没有测试任何 8-bit Qwen。该表在部分公开中文集上给出 Qwen/Whisper 的 WER：AISHELL-2 `2.71 / 3.15`、FLEURS-zh `2.41 / 2.88`、CV-zh `5.35 / 6.89`、CV-zh-TW `3.77 / 5.59`，粤语 FLEURS `3.98 / 5.79`。[Qwen 官方评测表](https://github.com/QwenLM/Qwen3-ASR#evaluation)

这可作为“Qwen 1.7B 值得进入准确率候选”的发布方证据，**不能**得出“Qwen 8-bit 必然优于 Paraformer Q8 或任意 Whisper”的结论，原因是：

- Qwen 的数字是发布方自测、BF16、vLLM、贪心解码和未指定语言；不是目标的 8-bit MLX/runtime。
- 表内是 Whisper `large-v3`，不代表 `turbo`、`medium` 或任意第三方 Whisper 运行时。
- Paraformer 不在该表；本仓库的 Paraformer Q8 使用 AISHELL-1、FLEURS、字符级规范化和 sherpa-onnx，不能直接与 Qwen 表混排。参见现有 [Paraformer ONNX benchmark](/Users/yugonglian/github/Saymore/docs/research/paraformer-onnx-benchmark.md)。

## 建议决策

1. **近期默认：Paraformer Q8。**它最符合“中文优先、本地、按键说话、尽快给出稳定最终文本”的首版目标；已有实际资源和准确率基线，不依赖未验证的第三方量化。
2. **高级实验：锁定一个 Qwen3-ASR-1.7B 8-bit MLX 制品。**只在 16 GB 以上 Apple Silicon 等实际门槛通过后展示，并把“实验性、约 2.5 GB 下载、量化来源”明确给用户。它尤其应接受繁体中文、粤语、方言、嘈杂音频和中英混说测试。
3. **Whisper 保留为对照和国际化备选。**若引入，先固定如 `large-v3-turbo` 或 `large-v3`、精确转换和运行时；不以“Typeless 可能使用”作为采纳理由。

在改变默认模型前，三者应在同一台目标设备、相同 VAD/切分、相同词典提示、相同文本规范化下比较：普通话、台湾繁中、粤语/目标方言、中英混说、专名数字、近远场噪声；报告 CER/WER、关键实体错误、首字 P50/P95、结束到最终文本 P50/P95、RTF、冷启动和峰值内存。量化版本必须与 BF16 基线成对报告，不得借用原始权重成绩。
