# Whisper large-v3-turbo INT8 制品锁定与 macOS Adapter 建议

调研日期：2026-08-03

范围：为 Saymore 的 macOS 本地听写锁定可由现有 `sherpa-onnx 1.13.4` Rust
运行时直接加载的 Whisper large-v3-turbo 制品，并核查文件身份、量化方式、
下载恢复能力、许可、来源链与中国大陆下载风险。外部事实只采用 OpenAI、
sherpa-onnx、Hugging Face 和 GitHub 的官方源码、API、发布页与许可证正文。

## 结论

**Mac Adapter 技术验证锁定 `csukuangfj/sherpa-onnx-whisper-turbo` revision
`2ca6ff69fc878651b770880507669577ac41c2ff` 的两份 INT8 ONNX 文件和一份词表。**
sherpa-onnx 官方 Whisper 导出文档把 `turbo` 明确指向这个仓库；该 revision 是仓库
当前唯一包含模型制品的提交，标题为 `upload turbo`，时间为 2024-10-02。
[sherpa-onnx 官方可用模型表](https://k2-fsa.github.io/sherpa/onnx/pretrained_models/whisper/export-onnx.html#available-models)
[Hugging Face 提交 API](https://huggingface.co/api/models/csukuangfj/sherpa-onnx-whisper-turbo/commits/main)

锁定的运行文件合计 **1,036,613,791 字节，即 988.59 MiB（0.97 GiB）**。因此
Saymore 当前界面中的 `3.1 GB` 下载量不能用于这个 INT8 方案，应改成约 `989 MB`；
运行内存也不能继续沿用未注明来源的估计值，必须以 Apple Silicon 真机峰值 RSS
重新标定。

这个结论可立即用于 Saymore 第一阶段的固定 Hugging Face 下载、Adapter 开发和
macOS 真机测试。正式镜像到 Saymore 自有 OSS 前仍需补一份可审计的转换来源清单：
转换仓库没有模型卡或独立许可证文件，sherpa-onnx 当时的自动导出工作流又使用了
未固定版本的
`openai-whisper`。模型字节已经可以用下表哈希精确识别，但仅凭现有仓库
不能重建一条完全可复现的转换工具链。

## 锁定的运行文件

固定文件树：
[Hugging Face revision](https://huggingface.co/csukuangfj/sherpa-onnx-whisper-turbo/tree/2ca6ff69fc878651b770880507669577ac41c2ff)。
两个 ONNX 的 SHA-256 与精确大小来自该固定 revision 的 LFS/Xet 对象元数据；
`turbo-tokens.txt` 从固定 revision 下载后本地计算 SHA-256。
[Hugging Face 模型 API](https://huggingface.co/api/models/csukuangfj/sherpa-onnx-whisper-turbo?blobs=true)

| 文件                      |        字节 | SHA-256                                                            | 固定下载地址                                                                                                                                 |
| ------------------------- | ----------: | ------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------- |
| `turbo-encoder.int8.onnx` | 674,716,297 | `b02dcdf54f348741e93fe732b67d933c8dcb6735655f710640143081db38878b` | [下载](https://huggingface.co/csukuangfj/sherpa-onnx-whisper-turbo/resolve/2ca6ff69fc878651b770880507669577ac41c2ff/turbo-encoder.int8.onnx) |
| `turbo-decoder.int8.onnx` | 361,080,764 | `20accd02388482eb3a46bd615631adfdc85e1eb2c7db9ea3f02a40ffe6b81547` | [下载](https://huggingface.co/csukuangfj/sherpa-onnx-whisper-turbo/resolve/2ca6ff69fc878651b770880507669577ac41c2ff/turbo-decoder.int8.onnx) |
| `turbo-tokens.txt`        |     816,730 | `b34b360dbb493e781e479794586d661700670d65564001f23024971d1f2fa126` | [下载](https://huggingface.co/csukuangfj/sherpa-onnx-whisper-turbo/resolve/2ca6ff69fc878651b770880507669577ac41c2ff/turbo-tokens.txt)        |

下载 manifest 必须保存 revision、相对路径、精确字节数和 SHA-256。不要使用
`/resolve/main/`，也不要把 Hugging Face 跳转后的签名 CDN URL
保存为下载源；重试时 应重新请求上表的固定 revision URL。

仓库还包含 FP32 / 外部权重形式的 `turbo-encoder.onnx`、`turbo-encoder.weights`
和 `turbo-decoder.onnx`，合计约 3.0 GiB。INT8 Adapter
不需要这些文件，不应下载或混装。运行时的三个文件名必须 匹配上表，避免把 FP32
encoder 和 INT8 decoder 组成未经验证的混合配置。

## 模型身份与量化方式

OpenAI 官方 Whisper 将 `turbo` 和 `large-v3-turbo` 映射到同一个 checkpoint：
`large-v3-turbo.pt`，官方 URL 路径内固定的 SHA-256 是
`aff26ae408abcba5fbf8813c21e62b0941638c5f6eebfb145be0c9839262a19a`，文件大小为
1,617,941,637 字节。它是约 809M 参数的 `large-v3` 优化变体；OpenAI 说明其转录
速度更快、精度只有少量下降，但明确指出 `turbo` **没有为翻译任务训练**，即使要求
`translate` 也会返回原语言。
[OpenAI checkpoint 映射](https://github.com/openai/whisper/blob/main/whisper/__init__.py#L68-L69)
[OpenAI 模型表与 Turbo 限制](https://github.com/openai/whisper#available-models-and-languages)

sherpa-onnx 在加入 Turbo 支持的提交中通过 `whisper.load_model("turbo")` 载入
OpenAI checkpoint，导出 encoder、decoder 与词表，并对 encoder 和 decoder 的
`MatMul` 权重执行 ONNX Runtime dynamic quantization，`weight_type` 为
`QuantType.QInt8`。因此这里的“INT8”是 **MatMul 权重动态 QInt8**，不是整个计算图
所有张量均为 8-bit，也不应宣称为 Apple Neural Engine 专用量化。
[sherpa-onnx Turbo 导出提交](https://github.com/k2-fsa/sherpa-onnx/commit/66feecb2b55917788a6852e23cdbbd9489b61d30)
[固定导出脚本](https://github.com/k2-fsa/sherpa-onnx/blob/66feecb2b55917788a6852e23cdbbd9489b61d30/scripts/whisper/export-onnx.py)

上述事实说明该制品适合 sherpa-onnx 的 Whisper Adapter，但不能从第三方转换仓库的
文件名反向证明每个 ONNX 张量都来自官方 checkpoint 的某个可复现构建。正式自有 OSS
包建议记录：官方 checkpoint URL 与 SHA-256、转换脚本 commit、Python/PyTorch/
ONNX/ONNX Runtime 精确版本、转换命令、输出三文件哈希以及两份许可证。

## 与现有 Rust 运行时的契合方式

Saymore 当前固定 `sherpa-onnx = 1.13.4`。该版本 Rust API 已提供
`OfflineRecognizer`、`OfflineRecognizerConfig` 和 `OfflineWhisperModelConfig`，
后者包含 encoder、decoder、language、task、tail paddings 和时间戳开关；共享模型
配置包含 tokens、线程数和 provider。这与锁定制品的三文件结构直接对应。
[sherpa-onnx 1.13.4 Rust 源码](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.4/sherpa-onnx/rust/sherpa-onnx/src/offline_asr.rs)

但 Whisper 与已完成的 Paraformer Adapter 有一个产品语义上的根本区别：
sherpa-onnx 官方把 Whisper 明确列为 **non-streaming speech recognition**。
[sherpa-onnx Whisper 文档](https://k2-fsa.github.io/sherpa/onnx/pretrained_models/whisper/index.html)

首个 Mac Adapter 应采用以下边界：

1. Provider 加载一次 `OfflineRecognizer`，每次按键听写创建独立 session。
2. session 在 `push_audio` 中只缓冲 16 kHz 单声道 PCM；`finish` 时创建
   `OfflineStream`、一次性送入音频并解码，`cancel` 直接丢弃缓冲。
3. 首版不伪造 partial。对不断增长的整段音频反复离线重解码会重复大量计算，并会让
   partial / final 的稳定性难以保证。
4. 默认 `task = "transcribe"`；首轮探针用空语言值让模型自动识别语言，再比较将
   `language = "zh"` 固定后的中英混输质量和延迟。不要向用户提供 Turbo 翻译选项。
5. macOS 第一轮固定 `provider = "cpu"` 并测量；CoreML 等 execution provider
   只有在 Apple Silicon 真机验证算子兼容、结果一致和峰值内存后才能启用。
6. 需要覆盖短音频、空音频、尾部静音、取消、连续多次开始/结束、超过 30 秒的听写、
   中英混输和模型加载失败。sherpa-onnx 的转换路径移除了固定 30
   秒输入限制，但这不 等于长听写已经满足 Saymore 的延迟和内存目标。

由于离线解码会在松开快捷键后才给出结果，首版体验与 Paraformer 边说边出字不同。
这是模型/运行时契约，不应通过 UI 状态掩盖。若最终验收要求实时 partial，需要另行
设计 VAD 分段和已确认文本拼接策略，而不是在本次 Adapter 中隐式实现。

## 下载源与中国大陆风险

2026-08-03 从测试网络对三个固定 Hugging Face URL 请求 `Range: bytes=0-0`：

- 三个文件均返回 `206 Partial Content`，并给出正确的总字节数；
- encoder 和 decoder 先由 `huggingface.co` 跳转到 `us.aws.cdn.hf.co` 的 Xet 签名
  URL，实际 CDN POP 为 AWS 亚太区域；
- tokens 经 Hugging Face resolve cache 返回，同样支持 Range。

这证明固定 URL 可用于 Saymore 已有的断点续传、重试、临时文件、大小校验、SHA-256
校验和原子激活流程，但不构成中国大陆可用性承诺。主站和实际对象存储仍是境外服务，
大文件合计接近 1 GiB，失败概率和完成时间都明显高于 226 MiB 的 Paraformer。

sherpa-onnx 官方文档同时给出 `hf-mirror.com` 链接，但调研时该域名对固定文件返回
`308` 并重新跳回
`huggingface.co`，没有形成独立下载链路。因此不能把它当成中国大陆
备用源。第一阶段按既定方案使用固定 Hugging Face URL；迁移自有 OSS 时保持文件字节
与哈希不变，只替换 manifest 中的源地址。

下载器还应在开始前按 1,036,613,791 字节加安全余量检查磁盘空间。并行下载 encoder
和 decoder 可以保留，但两个大文件同时写盘会增加瞬时带宽和磁盘压力；已有逐文件
`.part` 恢复与每文件重试语义可以直接复用。

## 为什么首版不使用 GitHub Release 压缩包

sherpa-onnx 官方 `asr-models` release 另有
[`sherpa-onnx-whisper-turbo.tar.bz2`](https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-whisper-turbo.tar.bz2)，
大小 563,790,207 字节（537.67 MiB），当前 `checksum.txt` 记录 SHA-256 为
`b11acbbcd660b44a8e0df33724feb5aaa709cf65668f2823d59f656312544f22`。
[GitHub Release API](https://api.github.com/repos/k2-fsa/sherpa-onnx/releases/tags/asr-models)
[官方 checksum.txt](https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/checksum.txt)

它可以显著减少网络字节，但当前 release API 标记 `immutable: false`，资产本身没有
API digest，而且使用它需要新增 bzip2/tar 解包、归档路径校验和解包原子性。Saymore
现有 Paraformer 下载器已经按固定 revision 逐文件校验，首阶段复用这条路径更小、更
容易验证。迁移自有 OSS 时可以自己生成只含锁定三文件、许可证和 provenance
manifest 的压缩包，并为整个包及解包后文件同时固定 SHA-256。

## 许可与再分发

OpenAI 明确声明 Whisper **代码和模型权重均按 MIT License 发布**；MIT 允许使用、
修改和再分发，但副本或实质性部分必须保留 OpenAI 版权和许可声明。
[OpenAI 许可说明](https://github.com/openai/whisper#license)
[OpenAI MIT 正文](https://github.com/openai/whisper/blob/main/LICENSE)

sherpa-onnx 代码和转换脚本使用 Apache-2.0。
[sherpa-onnx v1.13.4 许可证](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.4/LICENSE)
固定的 Hugging Face 转换仓库没有 README、模型卡或 LICENSE，因此 Saymore 不应仅
依据仓库页面自动生成许可结论。自有 OSS 分发包至少应附：OpenAI MIT、sherpa-onnx
Apache-2.0、原始 checkpoint 身份、转换工具身份、转换说明和三文件哈希。

与 Paraformer 不同，这里没有 FunASR 模型许可中的商业/再分发歧义；当前主要缺口是
转换 provenance，而不是取得 OpenAI 的额外商业授权。完整背景见
[本地 ASR 模型再分发许可核查](./asr-model-redistribution-license-check.md)。

## 明确建议

1. 立即把上表三文件加入与 Paraformer 相同结构的固定 manifest，状态设为
   `development_only / provenance_review_required`。
2. 复用现有 macOS 下载、Range 恢复、最多三次自动重试、逐文件大小/SHA-256、临时
   目录和原子激活基础设施；不要复用 Paraformer 的在线识别会话。
3. 新建 Whisper offline Adapter，完成真实音频探针后再开放下载按钮。由于下载接近
   1 GiB，必须先证明 Apple Silicon 上的模型加载、峰值
   RSS、短句完成延迟和连续会话 可接受。
4. UI 下载体积改为约 `989 MB`；Apple Silicon 真机使用生产 Adapter 冷加载并连续
   完成两次识别的峰值 RSS 为 `2,309,226,496` 字节（约 `2.15 GiB`），UI 运行内存
   估算值标为约 `2.2 GB`。
5. 第一阶段仍从 Hugging Face 固定 revision 下载。自有 OSS 就绪后只迁移完全相同的
   字节；若决定自行重导出，则把它视为新的模型
   revision，重新跑质量、性能和哈希验收。
