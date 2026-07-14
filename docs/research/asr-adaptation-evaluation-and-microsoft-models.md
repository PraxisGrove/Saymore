# 本地 ASR 的适配、评测与微软模型核查

调研日期：2026-07-15
范围：Saymore 的本地听写模型组合、领域适配，以及用户提到的“微软 Whisper ASR”。本文的上游事实只引用模型发布者、微软或 OpenAI 的一手资料；明确标为“建议”的内容是面向 Saymore 的工程判断。

## 先给结论

1. **可以做领域适配，不能把“从头自训基础 ASR”当作近期目标。**有真实录音、逐字标注和 NVIDIA 训练资源时，Paraformer、SenseVoice 和 Qwen3-ASR 都有发布方提供的训练/微调入口。先用热词、个人词典、格式化和端点策略解决问题；只有可复现地证明剩余错误来自声学/语言模型时，再微调。
2. **CER/WER 是必要但不充分。**它们衡量标准化文本的编辑距离，却不能衡量专名、数字、幻觉、流式延迟、模型资源和最终“能否放心投递到输入框”。评测必须按场景拆分，并记录产品指标。
3. **SenseVoiceSmall 不应被宣称为比 Paraformer 更强或更差。**发布方没有给出两者在相同语料、切分、解码和文本规范化下的对照。对“普通话、低延迟、边说边出字”，Paraformer streaming 是更自然的默认；SenseVoice 的合理候选定位是粤语及英/日/韩、整段最终转写或情绪/事件标签。若 Saymore 不提供这些能力，首批 3--4 个模型可以不加入它。
4. 用户可能说的是两个不同的微软发布物：**`microsoft/paza-whisper-large-v3-turbo` 是 OpenAI Whisper 的地区语言微调版，不是新的通用模型；`VibeVoice-ASR` 才是微软的新 ASR。**两者都不应取代 Saymore 的端侧中文实时默认模型。

## “微软 Whisper”到底是哪一个

| 名称 | 它是什么 | 对 Saymore 的结论 |
| --- | --- | --- |
| `microsoft/paza-whisper-large-v3-turbo` | 微软在 `openai/whisper-large-v3-turbo` 上微调的 ASR，目标是斯瓦希里语、卡伦金语、基库尤语、卢奥语、马赛语和索马里语；0.8B、MIT。它仍是 Whisper 的自回归 encoder-decoder，并要求长音频切分。 [官方模型卡](https://huggingface.co/microsoft/paza-whisper-large-v3-turbo) | 这很可能是“微软发布了 Whisper”的来源。它证明 Whisper 可被领域/语言微调，但对中文、粤语和桌面实时听写没有直接价值。 |
| `VibeVoice-ASR` | 微软的新长音频结构化转写模型：最长 60 分钟单次输入，联合给出转写、说话人和时间戳，可给热词，支持 50+ 语言和 code-switching。 [官方文档](https://github.com/microsoft/VibeVoice/blob/main/docs/vibevoice-asr.md) | 适合会议、访谈、播客转写的研发候选，不是按键听写的本地下载模型。 |
| Windows 的旧 “Whisper” | 这是微软 2008 年就存在的 Windows Highly Intelligent SPEech Recognizer，和 OpenAI Whisper 无关。 [微软项目页](https://www.microsoft.com/en-us/research/project/whisper-windows-highly-intelligent-speech-recognizer/) | 不纳入当前模型比较。 |

### VibeVoice-ASR 值不值得上

它不是 Whisper 的小升级，而是服务于另一类任务。官方公布的结果以会议/多说话人长音频为主，使用 DER、cpWER、tcpWER 和 WER；例如 AISHELL-4 的 WER 为 21.40，AliMeeting 为 27.40。这个结果支持它“值得做会议转写评测”，但**不能**和单人、短句、流式端点下的 Paraformer CER 直接比较。 [官方结果表](https://github.com/microsoft/VibeVoice/blob/main/docs/vibevoice-asr.md#results)

本地产品的阻碍也很明确：官方仓库将它称为 `VibeVoice-ASR-7B`，但官方 Hugging Face 卡当前列为 9B BF16 参数，二者命名/计数不一致，应以固定 revision 和文件哈希而非“7B”采购；模型卡显示 9B BF16，仓库的安装路径推荐 NVIDIA CUDA Docker。 [仓库](https://github.com/microsoft/VibeVoice) [官方模型卡](https://huggingface.co/microsoft/VibeVoice-ASR) 这意味着仅权重已经是约 17 GiB 量级，并没有发布方支持的 Apple Silicon/Windows CPU 小型运行时或端侧量化交付。

它有官方 LoRA 微调脚本，接受带说话人、时间段和可选上下文的真实音频标注；示例使用 BF16 和 GPU。 [官方微调指南](https://github.com/microsoft/VibeVoice/blob/main/finetuning-asr/README.md) 这适合有 NVIDIA 服务端的会议产品研究，**不适合 Saymore 第一阶段的客户端常驻模型**。微软也明确说未经进一步测试和开发，不建议直接用于商业或真实世界应用。 [仓库的风险说明](https://github.com/microsoft/VibeVoice#%EF%B8%8F-risks-and-limitations)

## SenseVoiceSmall 与 Paraformer：不该靠传闻做取舍

FunASR 的模型表把 `SenseVoiceSmall` 列为 234M、普通话/粤语/英语/日语/韩语及情绪/事件任务；`Paraformer-zh-streaming` 是 220M、中文/英文的流式 ASR。 [官方模型表](https://github.com/modelscope/FunASR#model-zoo) SenseVoiceSmall 是非自回归整段模型；其上游 README 提供 ONNX、GGUF/llama.cpp 路线，并明确说社区的分块“伪流式”会以准确率换取流式效果。 [官方 README](https://github.com/FunAudioLLM/SenseVoice#whats-new)

SenseVoice 发布方展示的是其与 Whisper 的比较，并称中文、粤语有优势；**没有**发布与 Paraformer 的同条件结果。 [SenseVoice benchmark](https://github.com/FunAudioLLM/SenseVoice#benchmarks) 因此“SenseVoice 不太行”与“它一定不如 Paraformer”都不是可成立的技术结论。

Saymore 的产品决策应当是：

- 普通话实时默认：保留 `Paraformer-zh-streaming`。它的分块 cache 接口已经匹配首字延迟和 partial result 的产品契约。
- 只有当目标集显示它在**粤语或英/日/韩**明显增益，或产品确实需要情绪/事件标签时，再提供 `SenseVoiceSmall`，并明确它是“最终转写”而不是 Paraformer 的替代流式档。
- 若首批只留三个模型，我会选 Paraformer、Whisper（国际化基线）和 Qwen3-ASR（准确率实验），**先不放 SenseVoice**；将其作为有明确粤语需求时的第四候选。GGUF、ONNX 或代码许可不能替代对实际分发权重许可的逐个核验。 [SenseVoice 许可说明](https://github.com/FunAudioLLM/SenseVoice#license)

## 能做到什么程度的“改进/自训练”

这里必须区分三个层级：

| 层级 | 解决什么 | Saymore 是否应做 |
| --- | --- | --- |
| 推理适配 | 热词、个人词典、术语替换、数字/日期/版本号 ITN、VAD 和端点、纠错 UI | **先做。**成本最低，能够回滚，且不会破坏基础模型的泛化能力。VibeVoice 本身也把自定义热词作为提高领域专名准确率的路径。 [官方文档](https://github.com/microsoft/VibeVoice/blob/main/docs/vibevoice-asr.md#-key-features) |
| 微调 / SFT | 目标口音、麦克风、噪声、行业术语和固定表达的转写 | **可做。**需要合法取得的真实音频和逐字人工标注，以及独立测试集。 |
| 从头预训练 | 重建通用声学/语言能力 | **不做。**这不是几个模型调优的延伸，而是数据、训练集群、研究与持续评测项目。 |

### 上游对微调的支持与现实优先级

| 家族 | 发布方支持 | 对 Saymore 的建议 |
| --- | --- | --- |
| Paraformer | FunASR 提供 `paraformer-zh` 训练入口、JSONL 数据清单和单/多 GPU 训练流程。 [官方教程](https://github.com/modelscope/FunASR/blob/main/docs/tutorial/README.md#model-training-and-testing) | 最先尝试。训练后的权重必须重新导出到实际 ONNX/量化运行时，并做量化前后完整回归；不要直接拿训练框架中的分数宣称客户端质量。 |
| SenseVoiceSmall | 发布方提供数据转换与 `finetune.sh`；数据可含语言、情绪、事件标签，也可只含音频与文本。 [官方微调说明](https://github.com/FunAudioLLM/SenseVoice#finetune) | 只为明确的粤语/五语或标签需求投入。不要为了“补齐模型目录”训练它。 |
| Qwen3-ASR-1.7B | 官方提供 JSONL 音频-文本 SFT 脚本、单/多 GPU `torchrun`、BF16/FlashAttention 路线。 [官方微调 README](https://github.com/QwenLM/Qwen3-ASR/blob/main/finetuning/README.md) | 有价值但排在 Paraformer 后。先在服务器 GPU 验证；当前客户端 Q8 变体需要独立转换、兼容性和质量验收，不能把 BF16 微调成绩搬到 Q8。 |
| Whisper | OpenAI 官方仓库发布的是推理代码和权重；模型卡指向论文说明训练/评测，但没有官方训练脚本或受支持的微调流程。 [仓库](https://github.com/openai/whisper) [模型卡](https://github.com/openai/whisper/blob/main/model-card.md) | 可以使用第三方训练栈做 Whisper 微调，但那是自建训练工程，不应称为 OpenAI 官方能力。没有充分数据、评测与发布流程时，不作为首个项目。 |
| VibeVoice-ASR | 官方支持 LoRA，且给出单 GPU/多 GPU BF16 示例。 [官方指南](https://github.com/microsoft/VibeVoice/blob/main/finetuning-asr/README.md) | 仅会议/长录音服务端研究。不要把 7B/9B 级模型下载给普通桌面听写用户。 |

**设备与数据的务实门槛（工程估算，不是上游保证）**：Paraformer/SenseVoice 的 220--234M 级模型可从一张 24 GiB NVIDIA GPU、小 batch 和短音频开始；Qwen 1.7B 的完整 BF16 微调应预留 48 GiB 以上或采用经验证的参数高效方案；VibeVoice 更应作为服务器训练任务。训练不在 Q8 上完成，先以 FP16/BF16 训练，再导出和量化。

先建立数据而不是先租 GPU。每条录音都应有授权、准确逐字文本、设备/噪声/方言/语言标签；训练、开发、测试之间不得复用说话人或同源录音。一个小而干净的 20--50 小时集合足以建立有意义的开发/回归集；要承诺通用领域改善，通常需要更多且覆盖真实分布的数据。它不是发布方的最低数据量承诺，而是避免小集过拟合的产品规划起点。

推荐执行顺序：先冻结基线与测试集，加入可回滚的热词/词典/ITN，收集并标注用户明确授权的失败样本，先微调 Paraformer，验证导出与 Q8 后再评估 Qwen；SenseVoice 仅在其负责的语言集合中加入实验。每次只改变一个变量（模型 revision、训练集、量化或端点），否则无法解释改善来自哪里。

## 评测：CER/WER 之外必须验收什么

CER/WER 仍是核心：CER 更适合连续中文字符，WER 适合有稳定词边界的语言；两者均需先固定大小写、空格、标点、全半角、繁简、数字的规范化规则。微软的 PazaBench 也将 CER、WER 和逆实时率 RTFx 并列追踪，说明速度本身就是 ASR 选择指标。 [PazaBench 说明](https://www.microsoft.com/en-us/research/blog/paza-introducing-automatic-speech-recognition-benchmarks-and-models-for-low-resource-languages/)

但 Saymore 的验收集至少还应有：

| 维度 | 需要报告的指标 / 样例 | 为什么不能被 CER/WER 代替 |
| --- | --- | --- |
| 关键内容 | 人名、产品名、命令、否定词、英文缩写的错误率；数字、日期、金额、单位、版本号的 exact-match | 一次专名或 `1.0`/`10` 错误可能比十个普通字错得更严重。 |
| 文本可投递性 | 标点、大小写、繁简、ITN、段落和中英混写的正确率；用户是否需手动改后才能发送 | 同样编辑距离的两段文字，前者可能可直接使用、后者不可。 |
| 异常输出 | 静音/噪声误转写率、无依据新增（hallucination）、重复循环、遗漏、语言误判 | Whisper 官方模型卡明确警告弱监督训练可能产生未在音频中说出的文字及重复输出。 [OpenAI 模型卡](https://github.com/openai/whisper/blob/main/model-card.md#performance-and-limitations) |
| 流式体验 | 首个稳定字 P50/P95、最终结果 P50/P95、partial 文本回改率、VAD 截断/误触发率 | CER 可以很低，但 2 秒后才出字或不断改写会让听写不可用。 |
| 性能与可靠性 | RTF、冷启动、峰值 RSS/VRAM、CPU/GPU/功耗、模型下载大小、加载/推理失败率与恢复 | 同一准确率模型可能根本无法在目标设备常驻。 |
| 鲁棒性与公平性 | 按设备、近远讲、噪声、口音/方言、性别/年龄、语速、语种切换拆分的全部指标 | 总平均数会掩盖某个关键用户群不可用。 |
| 长录音/多人（若支持） | 说话人 DER、按说话人 WER、时间戳偏差、cpWER/tcpWER | VibeVoice 的公开评测正是因此报告 DER、cpWER 和 tcpWER，而不是只报 WER。 [官方评测](https://github.com/microsoft/VibeVoice/blob/main/docs/vibevoice-asr.md#evaluation) |

测试集应分三层：公开集用于与历史基线对照；固定的、从不参与训练的产品验证集用于发布门槛；很小的已知失败回归集用于防止修一个热词坏掉另一个热词。每个结果都固定并披露模型 revision、量化、运行时、VAD/端点、文本规范化、硬件和测量日期。没有这些条件的 CER/WER 比较不应决定模型排名。

## 建议的首轮组合

不要同时“调优四个基础模型”。先把同一 Provider 契约、模型下载/校验、基准与错误标注流程做稳，再运行以下四条互不混淆的实验轨道：

1. `Paraformer-zh-streaming Q8`：中文实时基线与首个适配对象。
2. `Whisper large-v3-turbo`：国际化基线；不是中文默认模型。
3. `Qwen3-ASR-1.7B`：服务器 BF16 的准确率/方言研究，客户端量化另立验证任务。
4. `SenseVoiceSmall`：只在粤语、英/日/韩或需要标签能力时加入；否则把位置留给真实的用户需求，而不是凑四个模型。

VibeVoice-ASR 应单列为“服务端长音频/会议转写研究”，不进入这四个按需下载的端侧模型。这样每一个模型都有明确职责，训练数据与评分也不会被混成一个无法解释的总分。
