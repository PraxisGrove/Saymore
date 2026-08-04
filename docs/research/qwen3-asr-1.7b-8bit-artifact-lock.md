# Qwen3-ASR 1.7B INT8 制品锁定与 macOS 运行时结论

调研日期：2026-08-03

范围：为 Saymore 的 macOS 本地听写核查 Qwen3-ASR 1.7B 8-bit 制品，固定可下载
文件、revision、大小与 SHA-256，并确认现有 `sherpa-onnx 1.13.4` Rust 运行时是否
能够直接加载。外部事实只采用 Qwen、sherpa-onnx、转换项目、Hugging Face 和
ModelScope 的官方源码、文档、仓库 API 与许可证正文。

## 结论

**技术上不需要引入 MLX 或 Python sidecar。Saymore 当前固定的
`sherpa-onnx 1.13.4` 已经包含 Qwen3-ASR Rust API，可以直接加载三段式 ONNX
制品。** Qwen3-ASR 支持于 sherpa-onnx `1.12.34` 加入，其中明确包含 Rust API；
`1.13.4` 的 Rust 源码提供 `OfflineQwen3ASRModelConfig`，字段正好对应
`conv_frontend`、`encoder`、`decoder` 和 `tokenizer`。
[sherpa-onnx 1.12.34 changelog](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.4/CHANGELOG.md#11234)
[sherpa-onnx 1.13.4 Rust 配置源码](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.4/sherpa-onnx/rust/sherpa-onnx/src/offline_asr.rs)

但目前**没有 Qwen 官方发布并验证的 Qwen3-ASR 1.7B 8-bit 制品**。Qwen 官方只发布
原始 checkpoint，官方运行方式是 `qwen-asr` 的 Transformers / vLLM 后端；官方
README 和模型树没有 INT8、ONNX 或 MLX 文件。官方评测也明确使用 BF16，不能拿该
评测结果证明社区 INT8 转换的精度。
[Qwen3-ASR 官方仓库](https://github.com/QwenLM/Qwen3-ASR)
[Qwen 官方模型树](https://huggingface.co/Qwen/Qwen3-ASR-1.7B/tree/7278e1e70fe206f11671096ffdd38061171dd6e5)

第一阶段内部研发建议锁定 **sherpa-onnx 官方导出文档指向的** ModelScope 转换仓库
`zengshuishui/Qwen3-ASR-onnx` revision
`cb045ad80b8970c9d411d463e5b78991a566596c` 中的 1.7B INT8 文件。它是当前最短的
macOS 接入路线，且使用中国大陆 CDN；但它仍是社区转换制品，状态应标为
`development_only / provenance_and_quality_review_required`，不能写成“Qwen 官方
8-bit 版本”或声称“量化几乎无精度损失”。
[sherpa-onnx 官方导出说明](https://k2-fsa.github.io/sherpa/onnx/qwen3-asr/export.html)
[ModelScope 固定 revision](https://modelscope.cn/models/zengshuishui/Qwen3-ASR-onnx/files?version=cb045ad80b8970c9d411d463e5b78991a566596c)

## 锁定的运行文件

sherpa-onnx 官方预训练模型文档说明，Qwen3-ASR 的最小运行目录包含三个 ONNX 和
`merges.txt`、`tokenizer_config.json`、`vocab.json` 三个 tokenizer 文件。
[官方目录结构](https://k2-fsa.github.io/sherpa/onnx/qwen3-asr/pretrained.html#download)

下表的精确字节数与 SHA-256 来自 ModelScope 固定 revision 的仓库 API；三个小型
tokenizer 文件又从固定 URL 下载并在本机复算 SHA-256。三个 ONNX 的固定 URL 使用
`Range: bytes=0-0` 探测，均跳转到 `cdn-lfs-cn-1.modelscope.cn` 并返回
`206 Partial Content`、正确总大小和与仓库 API 一致的 `x-linked-etag`。

| 文件                              |          字节 | SHA-256                                                            | 固定下载地址                                                                                                                                      |
| --------------------------------- | ------------: | ------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| `model_1.7B/conv_frontend.onnx`   |    48,080,441 | `fa894a4ba53da6a4238f2a6ca0b09362e505d39cecbd646051b033e2e8d7e2fb` | [下载](https://modelscope.cn/models/zengshuishui/Qwen3-ASR-onnx/resolve/cb045ad80b8970c9d411d463e5b78991a566596c/model_1.7B/conv_frontend.onnx)   |
| `model_1.7B/encoder.int8.onnx`    |   314,222,162 | `436fbd910a0c8914851e5ac1354e807be9f283d08a5da728adaa609731c41469` | [下载](https://modelscope.cn/models/zengshuishui/Qwen3-ASR-onnx/resolve/cb045ad80b8970c9d411d463e5b78991a566596c/model_1.7B/encoder.int8.onnx)    |
| `model_1.7B/decoder.int8.onnx`    | 2,037,458,645 | `c43c853fa6e97d08365cb8a5502b360b595cd43c00dc60e4d8ca7cc18cad460b` | [下载](https://modelscope.cn/models/zengshuishui/Qwen3-ASR-onnx/resolve/cb045ad80b8970c9d411d463e5b78991a566596c/model_1.7B/decoder.int8.onnx)    |
| `tokenizer/merges.txt`            |     1,671,853 | `8831e4f1a044471340f7c0a83d7bd71306a5b867e95fd870f74d0c5308a904d5` | [下载](https://modelscope.cn/models/zengshuishui/Qwen3-ASR-onnx/resolve/cb045ad80b8970c9d411d463e5b78991a566596c/tokenizer/merges.txt)            |
| `tokenizer/tokenizer_config.json` |        12,487 | `4942d005604266809309cabc9f4e9cb89ce855d59b14681fdc0e1cc62ea26c4c` | [下载](https://modelscope.cn/models/zengshuishui/Qwen3-ASR-onnx/resolve/cb045ad80b8970c9d411d463e5b78991a566596c/tokenizer/tokenizer_config.json) |
| `tokenizer/vocab.json`            |     2,776,833 | `ca10d7e9fb3ed18575dd1e277a2579c16d108e32f27439684afa0e10b1440910` | [下载](https://modelscope.cn/models/zengshuishui/Qwen3-ASR-onnx/resolve/cb045ad80b8970c9d411d463e5b78991a566596c/tokenizer/vocab.json)            |

运行文件合计 **2,404,222,421 字节，即 2,292.85 MiB（2.24 GiB）**。下载器至少要按
该值加临时文件与安全余量检查磁盘空间。安装后建议保持以下目录形状：

```text
qwen3-asr-1.7b-int8-cb045ad8/
  conv_frontend.onnx
  encoder.int8.onnx
  decoder.int8.onnx
  tokenizer/
    merges.txt
    tokenizer_config.json
    vocab.json
```

ModelScope 仓库同时存在 FP32 encoder/decoder 及其 `.data` 文件，体积超过 9 GiB。
INT8 Adapter 不需要它们，不应下载，也不能把 FP32 和 INT8 文件混装。manifest 必须
保存固定 revision、相对路径、精确字节数和 SHA-256；不要使用 `master` 下载地址，
也不要保存跳转后的带时效 CDN 签名 URL。

## 制品来源与量化含义

Qwen 官方模型 `Qwen/Qwen3-ASR-1.7B` revision
`7278e1e70fe206f11671096ffdd38061171dd6e5` 标记为 Apache-2.0，原始模型树约 4.7
GB。Qwen 官方声明 1.7B 支持 30 种语言、22 种中文方言，以及离线和流式推理。
[Qwen 官方模型 API](https://huggingface.co/api/models/Qwen/Qwen3-ASR-1.7B)
[Qwen 官方模型说明](https://github.com/QwenLM/Qwen3-ASR#released-models-description-and-download)

ModelScope 转换仓库的 README 指向
[`Wasser1462/Qwen3-ASR-onnx`](https://github.com/Wasser1462/Qwen3-ASR-onnx)，
sherpa-onnx 官方文档也使用该项目作为导出代码。调研时该导出项目 HEAD 为
`0829a577ee741e408e65763dfe01ff4fe75408e9`。脚本对 encoder 的 `MatMul`、`Gemm`
和 `Linear` 权重使用 per-channel `QInt8` dynamic quantization，对 decoder 使用
per-channel `QUInt8` dynamic quantization；卷积前端仍是普通 ONNX。因此“8-bit”
准确含义是**主要矩阵权重的动态 INT8/UINT8 量化**，不是所有计算或缓存张量都只占 8
bit，也不是 Apple Neural Engine 专用模型。
[固定导出脚本](https://github.com/Wasser1462/Qwen3-ASR-onnx/blob/0829a577ee741e408e65763dfe01ff4fe75408e9/export_qwen3_asr_onnx.py)

转换脚本的 `--verify` 会比较 FP32 ONNX 与 PyTorch 的 encoder/decoder 数值，但
INT8 部分只检查 decoder 能否执行，没有把 INT8 输出与 FP32 输出比较，也没有跑
WER/CER 质量集。ModelScope 制品 README
同样没有记录基准结果、转换命令、依赖版本或原始 Qwen checkpoint
revision。因此当前资料**无法核实“8-bit 几乎没有牺牲性能/精度”**：
体积从官方原始模型约 4.7 GB 降到 2.24 GiB 是可核实的，识别质量是否接近 BF16 则
必须由 Saymore 的真实录音 A/B 验证。

另一个社区 Hugging Face 仓库
[`thieunv/sherpa-onnx-qwen3-asr-1.7B-int8`](https://huggingface.co/thieunv/sherpa-onnx-qwen3-asr-1.7B-int8)
也声明兼容 sherpa-onnx 1.13.4，但其 encoder 和前端哈希与 ModelScope 制品不同，且
调研时刚创建、下载量为零。不能把它作为同一模型的镜像或故障切换源；如果以后选择
它，必须作为新的 model revision 独立验收。

## 与 Saymore Adapter 的契约

首版可以直接使用 `OfflineRecognizer`：

1. `OfflineRecognizerConfig.model_config.qwen3_asr` 分别设置上表三个 ONNX 路径和
   tokenizer 目录；`max_total_len = 512`、`max_new_tokens = 512`、
   `temperature = 1e-6`、`top_p = 0.8`、`seed = 42` 与 sherpa-onnx
   官方示例一致。
2. macOS 首轮固定
   `provider = "cpu"`、`num_threads = 2`，真机测量后再决定线程数。不应仅凭 Apple
   Silicon 就假定 CoreML execution provider 可用。
3. Qwen 官方模型支持流式，但 sherpa-onnx 当前这里暴露的是 Offline recognizer。
   Saymore session 应像 Whisper 一样缓存 PCM，在 `finish` 时解码；若要实时
   partial，应另建 VAD/分段确认策略，不能把不断重跑整段离线解码伪装成流式。
4. 首版先不配置 hotwords。`1.13.4` 支持 Qwen3-ASR hotwords，可在三模型 A/B
   基线稳定 后复用个人词典，但必须单独验证热词是否造成普通词误替换。
5. Qwen 官方支持长音频，但固定 `max_total_len` 会限制单次生成长度。Saymore
   应使用 现有
   VAD/合理分段策略覆盖长听写，并测试跨段中文标点和中英边界拼接，不应静默 截断。

必须覆盖：纯中文、中英混输、数字与专有名词、背景噪声、空音频、尾部静音、取消、
连续多次开始/结束、超过一分钟长音频和模型加载失败。每条录音用同一 PCM 同时跑
Paraformer、Whisper 与 Qwen3-ASR；记录文字差异、完成延迟、冷加载、峰值 RSS
和稳定 内存。没有这组结果前，UI 只能写“INT8”，不能写“无损量化”或“效果最佳”。

## 下载源与发布风险

锁定 URL 在调研网络中由 ModelScope 跳转到中国大陆 CDN，所有大文件支持 Range，
因此可以复用 Saymore 已有的
`.part`、断点续传、最多三次重试、逐文件大小/SHA-256、临时目录和原子激活流程。与
Hugging Face 相比，这条源更适合中国大陆第一阶段下载，但 ModelScope 没有向
Saymore 提供可用性 SLA，仍不能代替未来的自有 OSS。

`decoder.int8.onnx` 单文件超过 2 GB。下载与校验代码必须使用 64-bit
长度，不能把大小 压入 32-bit signed integer；失败重试要重新请求固定 revision URL
以刷新 CDN 签名。
下载中、校验失败和删除损坏文件后的状态均应按模型独立维护，允许与其它模型并行。

## 许可与 provenance

Qwen 官方仓库和官方 Hugging Face 模型标记 Apache-2.0；Apache-2.0 要求再分发时
提供许可证副本、保留适用的版权/归属声明，并标明修改。Saymore 若镜像转换制品，应
随包保留 Qwen Apache-2.0、sherpa-onnx Apache-2.0、转换项目身份和量化说明。
[Qwen Apache-2.0 正文](https://github.com/QwenLM/Qwen3-ASR/blob/main/LICENSE)
[sherpa-onnx Apache-2.0 正文](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.4/LICENSE)

当前转换链仍有两个生产缺口：

- ModelScope 转换仓库没有独立 `LICENSE` 文件，README 只给出上游与实现链接；
- 转换产物没有记录所用 Qwen checkpoint 的固定 revision、导出脚本 commit、命令和
  依赖版本，无法从仓库元数据重建逐字节相同的制品。

因此内部研发可以按本文哈希锁定并验证，公开下载或迁移自有 OSS 前应完成一份
provenance manifest，并由项目方确认转换产物的再分发归属。最稳妥的生产路线是用
固定 Qwen checkpoint 和固定导出工具自行重导出、记录完整构建环境，然后将新输出
作为 Saymore 自己的 model revision 重新计算哈希和跑完整
A/B；不能假定自行重导出的 字节会与本文 ModelScope 文件相同。

## 实施决定

1. macOS 第一阶段采用本文 ModelScope 固定 revision 的六文件 INT8 ONNX 方案，
   继续使用 sherpa-onnx，不引入 MLX、Transformers、vLLM 或 Python sidecar。
2. 将模型状态标为 `development_only / provenance_and_quality_review_required`。
3. 接入通用下载与安装流程后，先完成真实模型冷加载和两次连续识别；峰值 RSS、延迟
   和模型输出均记录到新的基准文档。
4. 用相同录音做三模型 A/B，尤其验证用户关心的中文与中英混输。只有结果支持时才能
   对外描述 INT8 精度；否则回退到 0.6B INT8、FP16 或放弃该模型，而不是通过文案
   掩盖回退。
5. 自有 OSS 就绪后只镜像已经通过哈希、许可和质量验收的固定字节；任何重新导出都
   是新 revision，必须重新验收。
