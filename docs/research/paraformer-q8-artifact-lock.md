# Paraformer Q8 制品锁定与下载源核查

调研日期：2026-08-01

范围：为 Saymore 第一阶段的 macOS / Windows 本地实时听写锁定精确的 Paraformer Q8
研发制品，并核查候选版本、文件身份、下载恢复能力、中国大陆交付
路径和再分发前提。外部事实只采用 FunASR、ModelScope、sherpa-onnx、GitHub 与
Hugging Face 的发布页、API 或许可证正文。

## 结论

**研发适配锁定以下 sherpa-onnx 转换制品，不将它标记为 FunASR 官方模型制品：
`csukuangfj/sherpa-onnx-streaming-paraformer-bilingual-zh-en` revision
`8e40c43232a1c5c66c82111efc5820d3accca11b` 的三份 Q8 文件。**它与 Saymore
现有探针所使用的文件名完全一致，也与现有完整基准记录的模型尺寸相符；macOS 和
Windows 共用同一组模型文件，平台差异只属于 sherpa-onnx 运行库和打包。旧基准未
记录输入模型哈希，因此这里是目前证据下的最佳可重现锁定，不能反向声称已证明与
当时文件逐字节相同。

这个结论目前是 **`development_only`**，不是公共下载放行。第一阶段研发和内部测试
直接使用 Hugging Face 固定 revision；正式对外分发还缺少两项：

1. FunASR / 阿里对商业使用及 Saymore 自有 OSS/CDN 再分发的书面许可澄清；
2. 将锁定的三份文件放到中国大陆稳定下载源，并由客户端按大小和 SHA-256 校验。

不要把 Hugging Face 作为中国大陆用户的长期唯一生产下载源。当前请求虽然成功且支持
断点续传，但大文件实际跳转到美国区域 CDN；这只能证明测试网络在调研时可访问，
不能证明中国大陆各运营商和企业网络长期可用。

## Adapter 验证记录

2026-08-02 在 Apple Silicon macOS 上，锁定的三份文件通过大小和 SHA-256 校验后
被放入 Saymore 应用数据目录，未加入 Git 仓库。`ParaformerSpeechRecognizer` 使用
同一次模型加载连续完成两次生产契约会话，两次结果均为
`昨天是 monday today day is 礼拜二 the day after tomorrow 是星期三`，单次推理约
0.62 至 0.82 秒，未出现结尾重复或截断。确定性测试另外覆盖变化 partial、空
final、取消和 结束调用顺序。该记录不代替 Windows 真机和安装包验证。

## 来源层级

这里有两个不同的“上游”，不能混为一个官方版本：

1. **FunASR / 阿里官方原始模型**：ModelScope 上的
   `iic/speech_paraformer_asr_nat-zh-cn-16k-common-vocab8404-online`，提供原始
   PyTorch 权重、配置和词表。这是模型身份和训练来源的官方发布。
2. **sherpa-onnx 生态的运行时转换制品**：Hugging Face 上的
   `csukuangfj/sherpa-onnx-streaming-paraformer-bilingual-zh-en`，提供 Saymore
   当前 Rust/sherpa-onnx 运行时可直接加载的 Q8 ONNX 文件。sherpa-onnx
   官方文档列出这个仓库，但它不是 FunASR / 阿里官方账号发布的量化制品。

因此，本文的“锁定”只表示 Saymore 为了可重现研发而固定了一组精确字节，不表示
已经完成官方背书、来源链或商业再分发审查。

## 与 Saymore 相关的版本

sherpa-onnx 的流式 Paraformer 官方文档列出两组可用模型；每组都同时带 FP32 和
INT8
文件。[sherpa-onnx Paraformer 文档](https://k2-fsa.github.io/sherpa/onnx/pretrained_models/online-paraformer/paraformer-models.html)

| 候选             | 发布方声明的语言范围                                 | Q8 必需文件 | Saymore 结论                                                                 |
| ---------------- | ---------------------------------------------------- | ----------: | ---------------------------------------------------------------------------- |
| 中英双语流式版   | 中文、英文；文档另列普通话、河南话、天津话、四川话等 |  226.21 MiB | **首版锁定**；现有探针明确按它的文件结构加载，完整基准的名称与汇总尺寸也相符 |
| 中粤英三语流式版 | 中文、粤语、英文及文档列出的部分方言                 |  227.46 MiB | 暂不替换；尚未跑过 Saymore 同一套完整质量基准，且上游是个人 ModelScope 仓库  |

sherpa-onnx 另有多个**离线** Paraformer 包，例如 `paraformer-zh`、`zh-small` 和
英文版。它们不是同一个边说边出字的 Adapter 契约，不属于这次首版候选，不能把
“Paraformer 有多个离线发布物”误解为当前需要在多个 Q8 版本间选择。

三语版的转换 revision 是 `86931b152d37c908528b44d30b431c24a19770da`。其
ModelScope 上游页面标记为
`USER_UPLOAD`，并说明基于普通话、粤语和英语数据继续训练；这不是 FunASR 官方组织
发布的同一份中英模型。[三语上游元数据](https://www.modelscope.cn/models/dengcunqin/speech_paraformer-large_asr_nat-zh-cantonese-en-16k-vocab8501-online)

## 锁定的研发制品

转换发布仓库及不可变树：
[固定 revision 文件树](https://huggingface.co/csukuangfj/sherpa-onnx-streaming-paraformer-bilingual-zh-en/tree/8e40c43232a1c5c66c82111efc5820d3accca11b)。
sherpa-onnx 官方文档明确从这一转换模型提供下载与 macOS / Windows 命令，并要求
INT8 运行时使用 `encoder.int8.onnx`、`decoder.int8.onnx` 和 `tokens.txt`。

| 文件                |        字节 | SHA-256                                                            | 固定下载地址                                                                                                                                                  |
| ------------------- | ----------: | ------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `encoder.int8.onnx` | 165,462,184 | `81a70226a8934e6ed92aa1d4fc486b428b5398e2f2619ed4897b7294cab90e9a` | [下载](https://huggingface.co/csukuangfj/sherpa-onnx-streaming-paraformer-bilingual-zh-en/resolve/8e40c43232a1c5c66c82111efc5820d3accca11b/encoder.int8.onnx) |
| `decoder.int8.onnx` |  71,664,561 | `f3cca9f77bb9d93c8fcbfb63ae617b6b1ee96818df3aa3b151c40658fe38594f` | [下载](https://huggingface.co/csukuangfj/sherpa-onnx-streaming-paraformer-bilingual-zh-en/resolve/8e40c43232a1c5c66c82111efc5820d3accca11b/decoder.int8.onnx) |
| `tokens.txt`        |      75,756 | `59aba8873a2ed1e122c25fee421e25f283b63290efbde85c1f01a853d83cb6e6` | [下载](https://huggingface.co/csukuangfj/sherpa-onnx-streaming-paraformer-bilingual-zh-en/resolve/8e40c43232a1c5c66c82111efc5820d3accca11b/tokens.txt)        |

总大小是 237,202,501 字节（226.21 MiB）。两个 ONNX 的 SHA-256 来自固定 revision
的 LFS/Xet 对象元数据；`tokens.txt` 从固定 revision 下载后本地计算 SHA-256。

不要改锁到同作者较早的
[`streaming-paraformer-zh`](https://huggingface.co/csukuangfj/streaming-paraformer-zh)
仓库中的 `model_quant.onnx`。sherpa-onnx
发布者说明，文档所列转换包对模型添加了运行时 metadata 并将其改名为
`encoder.onnx`；因此较早转换仓库的 encoder 字节和哈希与 sherpa-onnx
文档所列包不同。[转换制品模型卡](https://huggingface.co/csukuangfj/sherpa-onnx-streaming-paraformer-bilingual-zh-en/blob/8e40c43232a1c5c66c82111efc5820d3accca11b/README.md)
Saymore 应锁定 sherpa-onnx 文档所列制品字节并重新做 Adapter
冒烟，不能仅因模型来源 相同就把两个 ONNX 文件视为可互换。

仓库现有
[`paraformer_onnx_probe.rs`](../../crates/infra/examples/paraformer_onnx_probe.rs)
也正是按上述三个文件名加载；既有完整评测记录 Q8 模型约 227.2 MiB、峰值 RSS 503
MiB，并且在 AISHELL-1 与 FLEURS 中文全集上没有可见的量化 CER 回退，见
[Paraformer ONNX benchmark](./paraformer-onnx-benchmark.md)。该报告没有保存三份
模型文件的 SHA-256；首次 Adapter 冒烟应先输出并核对本文哈希，再将新结果作为后续
回归基线。

## 为什么不直接下载上游 GitHub 压缩包

sherpa-onnx 文档推荐的
[GitHub Release 压缩包](https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-streaming-paraformer-bilingual-zh-en.tar.bz2)
当前资产 ID 为 `155855418`，大小是 1,047,319,737 字节（约 999 MiB）。它同时包含
FP32、Q8 和测试文件；用户为 226 MiB 的 Q8 运行文件下载近 1 GiB，不适合作为应用
内交付包。[GitHub 资产 API](https://api.github.com/repos/k2-fsa/sherpa-onnx/releases/assets/155855418)

还有两个身份问题：`asr-models` release 在
[GitHub Release API](https://api.github.com/repos/k2-fsa/sherpa-onnx/releases/130628817)
中是 `immutable: false`，并且 该旧资产没有 GitHub API 提供的 SHA-256
digest。上游同一 release 中的 `checksum.txt` 当前记录归档 SHA-256 为
`5462a1fce42693deae572af1e8c4687124b12aa85fe61ff4d3168bb5280e205f`，但 release
和 checksum 都不是不可变对象，仍不能只把 release 文件名 URL 当成制品身份。
开发阶段应使用上表的 commit URL
和文件哈希；生产阶段应打一个只含这三份文件及许可/
来源清单的新包，并为新包再生成独立 SHA-256。

## 下载源与中国大陆可达性

### 调研时的协议事实

2026-08-01 对三个候选来源做了只取首字节的 Range 探测：

| 来源                          | 最终存储域名                           | 结果                                          | 含义                                                                                 |
| ----------------------------- | -------------------------------------- | --------------------------------------------- | ------------------------------------------------------------------------------------ |
| Hugging Face 固定 commit 文件 | `us.aws.cdn.hf.co`                     | `206 Partial Content`，`Accept-Ranges: bytes` | 技术上可断点续传，但依赖境外主站和美国区域 CDN                                       |
| GitHub Release 压缩包         | `release-assets.githubusercontent.com` | `206 Partial Content`，`Accept-Ranges: bytes` | 技术上可断点续传，但包过大且同样是境外源                                             |
| ModelScope 官方原始模型       | `cdn-lfs-cn-1.modelscope.cn`           | `206 Partial Content`，`Accept-Ranges: bytes` | 国内 CDN 可断点续传，但仓库只有原始 PyTorch 权重，没有已锁定的 sherpa-onnx Q8 三文件 |

这些是协议和当前网络观测，不是地区 SLA。尤其不能从“本次能打开 Hugging Face”推出
“所有国内用户都能稳定下载 226 MiB”。应用下载器重试固定 Hugging Face URL 时，还
必须重新请求原始 `resolve/{commit}/{file}` 地址以获得新的 CDN 签名，不能持久化
会过期的跳转 URL。

ModelScope 官方中英流式模型目前的固定源 revision 是
`d0c35615b159110282736405f036c2d594a834be`，官方标签为 `v2.0.4`；其仓库给出
`model.pt`、配置和词表的文件大小与 SHA-256，并支持 `hub="ms"` 下载。
[ModelScope 官方模型](https://www.modelscope.cn/models/iic/speech_paraformer_asr_nat-zh-cn-16k-common-vocab8404-online)
但它不是 sherpa-onnx 可直接加载的 Q8 ONNX 包，所以“改用 ModelScope 地址”不能
单独解决生产交付。

### 建议的产品交付策略

1. 第一阶段由开发者从固定 Hugging Face revision 手动放置三文件，完成 macOS 与
   Windows Adapter 和会话语义验证。
2. 下载 manifest 将**模型身份**定义为三份路径、字节数和 SHA-256；下载 URL 只是
   可替换镜像，不能参与模型身份判断。
3. 取得再分发许可后，把完全相同的三份文件发布到 Saymore 控制的中国大陆 OSS/CDN；
   海外源可以保留固定 Hugging Face revision 作为备用。
4. 下载器需支持 Range、断点续传、源切换、逐文件 SHA-256、临时目录解压和校验后
   原子启用。任何一份文件不匹配都标记为损坏，不能尝试加载。

## 许可与来源阻塞

技术上可以锁定上述字节进行内部研发，但现在不能据此开放公共商业下载：

- 转换仓库模型卡标记 Apache-2.0，却没有在模型树中附独立 `LICENSE`；
- FunASR 官方
  [`MODEL_LICENSE 1.1`](https://github.com/modelscope/FunASR/blob/main/MODEL_LICENSE)
  一方面允许使用、复制、修改与分享模型及衍生品，另一方面又写有“仅作为参考和学习
  使用”，并要求注明出处、作者及保留模型名称；
- 转换模型卡只记录 ModelScope 上游地址，没有记录转换时采用的精确上游 revision，
  因而来源链仍不满足生产制品的完整 provenance 要求。

上线前需要 FunASR / 阿里书面确认：精确模型可否用于商业桌面产品、Saymore 可否把
Q8 转换制品镜像到自有 OSS/CDN，以及应随包附 Apache-2.0、`MODEL_LICENSE 1.1`
还是两者。许可核查的完整背景见
[本地 ASR 模型再分发许可核查](./asr-model-redistribution-license-check.md)。

## 下一步

研发 manifest 可以立即使用本文三文件记录，状态设为
`development_only / redistribution_review_required`。随后只实现“手动模型目录”的
Paraformer Adapter，并在 Apple Silicon、Intel Mac 和 Windows x64
上验证多次开始、
结束、取消和重新开始。许可澄清与国内镜像完成前，不打开面向用户的下载按钮。
