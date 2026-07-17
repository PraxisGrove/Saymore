# 本地 ASR 模型再分发许可核查

调研日期：2026-07-17

范围：核对 Saymore 候选模型 `paraformer-zh-streaming`、 `Qwen3-ASR-1.7B` 及其
8-bit 量化版本、OpenAI Whisper 官方权重是否可由 Saymore 镜像到自有 OSS
并向用户分发。只引用模型发布方与许可证正文等一手来源。

> 本文是工程侧许可核查，不是法律意见。最终上线应以所分发的精确制品、固定
> revision 中随附的许可文件为准；存在冲突或商业用途不明确时，应取得权利方的
> 书面确认。

## 结论

| 制品                                                          | 工程侧判断                                                                                                                                                                             | OSS 上架前提                                                                                                                 |
| ------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| **FunASR `paraformer-zh-streaming` 官方权重或其 Q8 衍生品**   | **暂缓直接镜像。**FunASR 的模型专项协议允许使用、复制、修改、分享权重及衍生品，但同时写有“仅作为参考和学习使用”；官方 Hugging Face 模型页却标为 Apache-2.0。两处官方信息存在实质歧义。 | 先向 FunASR/阿里取得可用于商业产品并可在自有 OSS 再分发的书面确认，同时确认应随包附带哪份许可。                              |
| **Qwen 官方 `Qwen3-ASR-1.7B` 原始权重**                       | **可以按 Apache-2.0 条件再分发。**官方模型卡将该模型标为 Apache-2.0，官方仓库提供 Apache-2.0 正文。                                                                                    | 附许可证；保留适用的版权、专利、商标和署名通知；若上游含 `NOTICE` 则一并处理；不得暗示获得 Qwen/阿里商标授权。               |
| **由 Saymore 从上述官方 Qwen revision 自行生成的 8-bit 权重** | **可以按 Apache-2.0 条件再分发。**量化后的制品应保守地按修改版/衍生制品处理。                                                                                                          | 除上述义务外，显著注明已由 Saymore 修改，记录原始 revision、量化方法、工具版本和原始/输出哈希。                              |
| **OpenAI 官方 Whisper 权重**                                  | **可以按 MIT 条件再分发。**OpenAI README 明确称代码和模型权重均以 MIT 发布。                                                                                                           | 在副本或实质性部分中保留 OpenAI 版权声明及完整 MIT 许可声明。                                                                |
| **由 Saymore 自行转换或量化的 Whisper 权重**                  | **可以按 MIT 条件再分发。**                                                                                                                                                            | 保留同一 MIT 版权与许可声明，并记录转换来源和哈希。                                                                          |
| **任意第三方 Q8/GGUF/ONNX 仓库**                              | **不能仅凭原模型许可直接放行。**上游许可只解决上游材料，不自动证明第三方上传者有权发布其改动，也不覆盖其新增文件、工具或依赖。                                                         | 逐仓库核验作者、来源 revision、转换过程、随附许可、NOTICE、文件哈希及运行时依赖；优先不用，改为 Saymore 从官方权重自行量化。 |

## 1. Paraformer：存在需要发布方澄清的冲突

FunASR 官方仓库的
[`MODEL_LICENSE 1.1`](https://github.com/modelscope/FunASR/blob/main/MODEL_LICENSE)
明确将“FunASR 软件”定义为**模型权重及其衍生品（包括微调模型）**，并允许在协议
条件下使用、复制、修改和分享。它要求注明出处及作者信息、保留相关模型名称；违反
条款会自动终止许可。该协议还称模型“仅作为参考和学习使用”，并规定后续修订会自动
生效。[协议原文](https://raw.githubusercontent.com/modelscope/FunASR/main/MODEL_LICENSE)

另一方面，FunASR 官方组织的
[`funasr/paraformer-zh-streaming`](https://huggingface.co/funasr/paraformer-zh-streaming)
模型页显示 `License: apache-2.0`，但其文件树没有随权重列出独立 `LICENSE` 文件。
因此不能在没有进一步确认的情况下自行选择更宽松的 Apache-2.0 标记，忽略专项模型
协议中的“参考和学习”措辞。

这也适用于 Paraformer Q8：专项协议明确覆盖权重“衍生品”，量化不会消除原权重的
许可条件。若当前 Q8 来自 sherpa-onnx 或其他转换发布者，还需独立核查该具体制品的
来源和附加许可。发布前应向 FunASR/阿里书面确认：

1. `paraformer-zh-streaming` 精确 revision 是否允许商业产品使用；
2. 是否允许 Saymore 将其原始或 Q8 转换制品托管到自有 OSS/CDN 并提供给终端用户；
3. 再分发时究竟适用 Apache-2.0、`MODEL_LICENSE 1.1`，还是两者均需附带；
4. 所需的产品内署名、模型名称、作者信息和许可证展示方式。

在获得答复前，工程状态应标为 `redistribution_review_required`，不应发布 Saymore
镜像下载地址。可继续本地适配与评测，但这不等于已获得产品再分发许可。

## 2. Qwen3-ASR-1.7B：官方源与自行 Q8 可以再分发

Qwen 官方 Hugging Face 模型仓库的
[`Qwen/Qwen3-ASR-1.7B` 模型卡](https://huggingface.co/Qwen/Qwen3-ASR-1.7B) 标为
`apache-2.0`，并明确发布模型架构与权重；其文件树显示官方发布物是原始
`safetensors` 分片，没有官方 8-bit checkpoint。
[官方文件树](https://huggingface.co/Qwen/Qwen3-ASR-1.7B/tree/main) Qwen3-ASR
官方代码仓库附带完整
[`Apache License 2.0`](https://github.com/QwenLM/Qwen3-ASR/blob/main/LICENSE)。

Apache-2.0 第 2、4 节允许复制、制作衍生作品及以源或目标形式分发。再分发需要：

- 向接收者提供 Apache-2.0 许可证副本；
- 对修改过的文件作醒目标记；
- 保留适用的版权、专利、商标和署名通知；
- 如果上游发布物含 `NOTICE`，按第 4(d) 节保留其中适用的署名；
- 注意第 6 节不授予商标权，只能合理说明来源。

义务以
[Apache License 2.0 正文第 4 节](https://www.apache.org/licenses/LICENSE-2.0#redistribution)
为准。

Saymore 从固定官方 revision 自行生成的 8-bit 权重不应冒充“Qwen 官方 Q8”。建议
显示为“Qwen3-ASR-1.7B 8-bit, converted by Saymore”，并在 OSS 制品内附：

```text
LICENSE
NOTICE              # 上游存在时保留；Saymore 可追加自己的署名
PROVENANCE.json      # 官方仓库、revision、原始文件及 SHA-256
CONVERSION.json      # 量化算法、工具/版本、参数、日期及输出 SHA-256
```

使用第三方 Q8 仓库时，必须同时满足 Apache-2.0 的上游条件和第三方改动的许可条件。
若第三方没有清晰来源、修改声明或许可文件，不应镜像到 Saymore OSS。

## 3. OpenAI Whisper：官方权重与自行转换均可再分发

OpenAI Whisper 官方 README 明确写明其**代码和模型权重**均按 MIT License
发布；官方 加载器还列出了 OpenAI 托管的各 checkpoint 下载地址和 SHA-256
路径，其中包括 `large-v3` 与 `large-v3-turbo`。
[Whisper 许可说明](https://github.com/openai/whisper#license)
[官方 checkpoint 列表](https://github.com/openai/whisper/blob/main/whisper/__init__.py)

[MIT 许可证正文](https://github.com/openai/whisper/blob/main/LICENSE)允许使用、复制、
修改、合并、发布、分发、再许可及销售副本，条件是在软件副本或实质性部分中保留
版权声明和许可声明。因此 Saymore 可以镜像官方 `.pt` 权重，也可以从固定官方权重
自行转换/量化后分发，但 OSS 制品必须附带 OpenAI 的 MIT 声明。

第三方 Whisper GGML/GGUF/量化仓库仍需单独核查。官方权重的 MIT 许可不自动覆盖
第三方转换代码、运行时、附加 tokenizer 文件或其他依赖；例如具体运行时与音频解码
库必须按它们各自的许可证履约。

## 发布清单建议

每个计划上架 OSS 的精确模型制品都应通过以下门禁：

1. 只从发布方官方仓库固定不可变 revision 获取原始权重；
2. 保存当时随附的模型卡、许可证和 NOTICE，而不是只记录网页上的 license 标签；
3. 优先由 Saymore 自行转换/量化，保存可复现命令、工具版本及输入输出哈希；
4. 为每个制品生成独立许可目录，不能用“运行时代码是 MIT/Apache”替代模型权重许可；
5. 发布前复核 tokenizer、词表、配置、运行时和解码依赖的许可证；
6. 禁止覆盖已发布 OSS 对象；新转换或新上游 revision 必须生成新制品版本；
7. Paraformer 在取得书面澄清前只允许研发使用，不进入 Saymore 公共下载清单。
