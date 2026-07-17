# 中文优先 ASR Model Hub

状态：MVP 后产品方向，不属于首版范围 日期：2026-07-13

## 1. 决策摘要

Saymore 继续首先是一个可靠的源码可用语音输入工具。MVP
只提供一个经过完整验证、由用户主动选择下载的推荐本地 ASR 模型，以及用于证明
Provider 边界的有限云端路径，不建设模型市场、社区排名或在线模型目录。

> 状态：长期规划，近期不实现，当前没有开发时间表。macOS
> 核心听写和发布链达到稳定状态后再重新评估。

在核心听写、投递和模型生命周期稳定后，Saymore 可以扩展为中文优先的 ASR Model
Hub：用户按语言、硬件、隐私和延迟需求，一键安装经过兼容性验证的本地 ASR
模型，也可以配置云端 ASR
Provider；社区通过公开、可复现的测试维护模型能力与兼容性信息。

一句话方向：

> Saymore 是面向中文用户的源码可用语音输入工具；它让经过验证的本地 ASR
> 模型可以被发现、比较、一键安装和可靠切换，同时保留云端 ASR API 作为可选路径。

Model Hub
是语音输入产品的后续差异化能力，不取代“说话后可靠输入文字”这一核心价值。

## 2. 为什么中文优先

现有开源桌面语音输入工具已经可以下载多个本地模型，但常见路径主要围绕不同大小的
Whisper 权重或 Whisper 与 Parakeet 的组合。例如 Handy 支持多档 Whisper 和
Parakeet，VoiceInk 使用 Whisper.cpp 与
Parakeet。仅增加多个下载按钮不能构成稳定差异。

Saymore 关注的是当前分散、部署方式不同、难以横向比较的中文 ASR 生态，包括
Paraformer、SenseVoice、Qwen3-ASR、Fun-ASR-Nano、GLM-ASR-Nano、FireRedASR、WeNet
和 PaddleSpeech
等模型或工具链。产品价值来自把这些模型转换为可理解、可验证、可恢复的桌面能力，而不是按模型产地建立排他目录。

因此使用“中文优先”，不使用“只支持国产模型”：

- 中文、句内中英混说、方言、专有词、数字和中文 ITN 是主要验收维度。
- Whisper、Parakeet 等模型仍可作为兼容选项和公开基准，不作为首页默认叙事。
- 模型是否进入 Hub
  取决于许可证、运行时安全、目标硬件表现和可复现测试，不取决于厂商国别。

## 3. 用户承诺与边界

Model Hub 可以承诺：

- 一键安装任何已经被当前 Saymore 版本声明兼容的模型变体。
- 安装前展示来源、精确版本、许可证、文件大小、数据边界和硬件要求。
- 下载、校验、加载、切换、回退和删除由统一模型生命周期管理。
- 本地模型和云端 Provider 使用同一能力词汇比较，但明确标识音频是否离开设备。
- 社区结果与 Saymore 验证结果分开显示。

Model Hub 不能承诺：

- 任意 Hugging Face、ModelScope 或用户提供的模型都能直接运行。
- 模型论文、模型卡或厂商宣传中的能力自动成为产品承诺。
- 下载量、评分或单一 WER/CER 可以代表所有设备和语言场景的质量。
- Saymore 对第三方模型拥有版权，或为上游模型提供通用安全保证。

## 4. 模型权重不等于可运行 Provider

不同模型家族需要不同运行时：

| 模型家族      | 典型运行时边界                             |
| ------------- | ------------------------------------------ |
| Whisper       | whisper.cpp、GGML 或 GGUF Adapter          |
| Paraformer    | FunASR、ONNX 或经过验证的原生 Adapter      |
| SenseVoice    | ONNX、GGUF 或 FunASR Adapter               |
| Qwen3-ASR MLX | MLX-Audio 或经过验证的原生 MLX Adapter     |
| Parakeet      | FluidAudio 或专用推理 Adapter              |
| 云端 ASR      | 各 Provider 独立的认证、流式协议和错误映射 |

社区提交一份模型元数据不能自动产生新的可执行运行时。Saymore
只把权重、分词器和配置等数据文件纳入按需下载；运行时代码和 Provider Adapter
必须随经过审查和签名的应用版本发布。客户端不得从模型条目下载并执行任意
Python、动态库、安装脚本或 `trust_remote_code` 代码。

这意味着用户看到的准确承诺是“Saymore-compatible models”，不是“所有 ASR models”。

## 5. 注册表与模型状态

Model Hub 建立在现有签名适配清单之上，不再发明第二套模型身份。每个条目至少包含：

```text
model_id
upstream_project
upstream_version
variant
runtime_adapter
artifacts[]
artifact_sizes[]
artifact_hashes[]
license
redistribution_policy
supported_platforms[]
minimum_hardware
recommended_hardware
capabilities
benchmark_results[]
verification_status
compatible_saymore_versions
```

用户可见状态分为：

- **Saymore Verified**：固定版本、许可证、供应链、能力契约和目标硬件测试均通过。
- **Community**：社区提交，元数据和基础安全检查通过，但未完成 Saymore 全量验收。
- **Experimental**：能够在指定环境运行，但质量、资源占用或稳定性尚未达到正式支持门槛。
- **Server-only**：需要 CUDA、独立推理服务器或不适合桌面常驻的硬件。
- **Incompatible**：与当前平台、硬件或 Saymore 版本不兼容，不能安装或启用。

未进入签名适配清单的手动导入模型继续标记为“未由 Saymore
验证”，不能参与官方排名。

## 6. 候选模型版图

以下只是后续验证队列，不是 MVP 承诺：

| 候选                    | 主要验证定位                   |
| ----------------------- | ------------------------------ |
| Paraformer streaming    | 中文实时、低延迟、两遍识别     |
| Qwen3-ASR-1.7B 量化变体 | 中文、多语言、方言、准确优先   |
| SenseVoiceSmall         | 轻量整句识别和低配设备         |
| Fun-ASR-Nano            | GPU 自部署、多语言和服务端路径 |
| GLM-ASR-Nano            | 中文、方言和量化部署           |
| FireRedASR              | 普通话、方言、英文和服务端路径 |
| WeNet、PaddleSpeech     | 流式工程生态和行业部署         |
| Whisper、Parakeet       | 国际兼容性和横向基准           |

模型名称不固定具体版本或量化格式。只有完成许可证审查、Adapter
实现和目标硬件基准后，某个精确变体才能进入签名适配清单。

## 7. 排名不能等同于下载量

Model Hub
将客观基准、兼容性、使用趋势和主观评价分开，不能合成一个不透明的“总分”。候选榜单包括：

- **热门趋势**：最近 7 天安装量。
- **持续使用**：最近 30 天选择加入遥测的活跃设备数。
- **普通话准确率**：固定公开测试集的 CER。
- **中英混说**：句内切换、英文缩写和技术术语测试。
- **方言表现**：按明确方言和测试集分别报告。
- **中文格式化**：数字、日期、金额、版本号、标点和 ITN。
- **设备速度**：按 M1 8 GB、其他 Apple Silicon、Windows CPU 和 NVIDIA GPU 分组。
- **资源效率**：模型大小、冷启动、峰值内存、CPU、GPU 和功耗。
- **社区评价**：主观评分和文字反馈，不混入官方 benchmark。

排名结果必须显示模型精确版本、量化变体、运行时、参数、设备、测试集版本和测量日期。跨设备、跨语言或跨模型版本的数据不得直接混排。

“每日更新”指每天生成一次可审计的排名快照，不要求实时刷新。没有足够样本的数据标记为不足，不用小样本制造排名。

## 8. Benchmark 原则

中文优先基准至少覆盖：

- 普通话安静近场。
- 噪声、远场和低音量。
- 中文句内夹英文术语。
- 英文句内夹中文名称。
- 人名、品牌名、缩写和生僻词。
- 数字、日期、金额、单位、版本号和否定词。
- 连续停顿、自我修正和重复表达。
- 明确标注的方言与地区口音。
- 30 秒连续讲话和产品允许的长口述。

质量指标至少报告
CER/WER、关键实体错误、重复、遗漏和无依据新增；性能指标至少报告首字延迟、结束延迟、实时率、冷启动、峰值内存和功耗。Saymore
必须发布 benchmark 命令、固定 fixtures
或可公开数据集引用，以及结果格式，使社区能够复现而不是只提交截图。

## 9. 下载、版本和供应链

- 应用安装包不包含 ASR 主权重，模型由用户确认后按需下载。
- 优先使用上游不可变版本；在许可证允许时可提供哈希相同的 ModelScope
  或国内对象存储镜像。
- 下载支持暂停、恢复、重试、大小与哈希校验，并使用临时文件和原子安装。
- 模型、运行时 Adapter 和 Saymore 应用分别版本化，兼容关系由签名适配清单声明。
- 新模型首次真实识别成功前保留旧模型；失败时自动回退。
- 用户可以删除权重并保留配置，也可以手动导入相同固定版本用于受限网络环境。
- 模型解析只接受对应 Adapter
  明确支持的安全数据格式；归档解压必须防止路径穿越和资源耗尽。
- 权重许可证与运行时代码许可证分别审查。引导用户从上游下载不免除许可证展示和使用条件审查。

## 10. 隐私与反滥用

模型排名不能破坏 Saymore 的 local-first 承诺：

- 安装统计、活跃使用统计和崩溃报告分别选择加入，默认关闭。
- 禁止上传音频、转录、词典、历史、热词、Provider 地址、请求或响应。
- 统计只允许模型 ID、精确版本、运行时版本、粗粒度设备档位、耗时和成功状态。
- 不上传稳定设备标识；公开结果只展示达到最小样本门槛的聚合数据。
- 下载量、活跃使用、benchmark 和社区评分分开，降低刷榜影响。
- 社区条目、镜像和评价需要审核、限流、重复检测和撤回机制。

## 11. 分阶段路线

### 阶段 0：MVP

- 只支持一个经过完整验证、由用户主动选择下载的推荐本地模型档位。
- 保留有限云端 ASR Provider，证明 Provider 边界和降级行为。
- 完成签名适配清单、按需下载、哈希校验、手动导入、删除和模型切换保护。
- 不提供 Model Hub 页面、社区提交、安装趋势、评分或在线排名。

### 阶段 1：静态模型目录

- 在首个推荐模型稳定后增加少量、由 Saymore 维护的兼容模型变体。
- 注册表仍随签名应用元数据或签名静态清单发布，不依赖账户或在线社区后端。
- 发布公开 benchmark CLI、结果格式和硬件档位。

### 阶段 2：社区注册表

- 允许通过代码审查和清单 PR 提交社区模型元数据与 benchmark 结果。
- 增加 Community、Experimental 和 Server-only 状态。
- 建立结果复现、许可证审查、撤回和安全响应流程。

### 阶段 3：托管 Model Hub

- 提供签名在线注册表、每日排名快照、选择加入的匿名使用趋势和社区评价。
- 保留离线静态清单和手动导入，在线服务不可成为本地听写的单点故障。
- 是否托管第三方权重、提供 CDN 或托管 ASR 服务分别决策，不能因 Model Hub
  自动获得授权。

每一阶段都以听写可靠性为前置条件。只要快捷键、录音、识别、投递或失败恢复仍不稳定，就不推进社区和排名能力。

## 12. 参考项目与模型

- [Handy](https://github.com/cjpais/Handy)：多档 Whisper 与 Parakeet
  的本地模型管理参考。
- [VoiceInk](https://github.com/Beingpax/VoiceInk)：Whisper.cpp 与 Parakeet 的
  macOS 语音输入参考。
- [Qwen3-ASR](https://github.com/QwenLM/Qwen3-ASR)
- [FunASR](https://github.com/modelscope/FunASR)
- [Fun-ASR-Nano](https://github.com/FunAudioLLM/Fun-ASR)
- [GLM-ASR-Nano](https://huggingface.co/zai-org/GLM-ASR-Nano-2512)
- [FireRedASR](https://github.com/FireRedTeam/FireRedASR)
- [WeNet](https://github.com/wenet-e2e/wenet)
- [PaddleSpeech](https://github.com/PaddlePaddle/PaddleSpeech)

这些项目的模型卡、代码仓库和厂商 benchmark 只用于建立候选集合。Saymore
的兼容性和质量声明仍以固定版本、目标硬件和自己的可复现验收结果为准。
