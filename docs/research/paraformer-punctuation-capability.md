# Paraformer 标点能力核查

调研日期：2026-08-03

## 结论

Saymore 当前固定的流式 Paraformer Q8 制品不能独立完成可靠的自动标点。固定
revision 的 Q8 运行文件只有 `encoder.int8.onnx`、`decoder.int8.onnx` 和
`tokens.txt`；当前 `ParaformerSpeechRecognizer` 只创建 sherpa-onnx
`OnlineRecognizer`，最终结果没有 经过独立标点恢复器。

FunASR 的 Paraformer 源码明确说明，Paraformer 需要额外配置
`punc_model="ct-punc"` 才能输出标点。sherpa-onnx
同样把标点定义为语音识别之后的独立文本处理阶段，而不是 Paraformer recognizer
的配置开关。

## 推荐方案

第一阶段为 Paraformer 增加 sherpa-onnx 官方中英 INT8 标点模型：
`sherpa-onnx-punct-ct-transformer-zh-en-vocab272727-2024-04-12-int8`。

- 该模型支持中文和英文；官方包中的 `model.int8.onnx` 约 72 MB。
- 它是离线标点模型，应在用户结束口述后对完整最终文本调用一次。
- partial 保持 Paraformer 的原始无标点文本，避免每次 partial
  都重跑完整文本并导致 标点跳动。
- 标点失败时应回退到原始识别文本，不应让口述会话失败。
- Mac 和 Windows 可以共用同一份标点模型；Saymore 当前固定的 sherpa-onnx 1.13.4
  Rust crate 已提供 `OfflinePunctuation`、`OfflinePunctuationConfig` 和
  `add_punctuation`。

sherpa-onnx 另有在线标点 API，但当前官方现成的在线模型只支持英文，不能作为中文
Paraformer 的实时标点方案。FunASR 有中文实时 CT-Transformer 路线，但其接口和模型
格式不属于 sherpa-onnx 当前中文在线标点契约，需要单独适配和验证。

## 固定制品证据

[固定 revision 文件树](https://huggingface.co/csukuangfj/sherpa-onnx-streaming-paraformer-bilingual-zh-en/tree/8e40c43232a1c5c66c82111efc5820d3accca11b)
没有标点模型或标点配置。sherpa-onnx 的
[流式 Paraformer 官方示例](https://k2-fsa.github.io/sherpa/onnx/pretrained_models/online-paraformer/paraformer-models.html)
对完整测试音频输出“昨天是 monday ... 是星期三”，JSON 的 `text` 和 `tokens`
均没有句法标点。固定
[`tokens.txt`](https://huggingface.co/csukuangfj/sherpa-onnx-streaming-paraformer-bilingual-zh-en/blob/8e40c43232a1c5c66c82111efc5820d3accca11b/tokens.txt)
只有一个 ASCII `.`，没有中文逗号、句号、问号、感叹号等完整标点集合；单个点号
不能证明模型具备标点恢复能力。

2026-08-03 还使用锁定的三份 Q8 文件、官方 `test_wavs/0.wav` 和 Saymore 当前生产
Adapter 做了两次连续会话。两次 final 都是：

```text
昨天是 monday today day is 礼拜二 the day after tomorrow 是星期三
```

所有 partial 也都没有标点。这是单个官方样本的制品核验，不是准确率评测；它用于
确认 Saymore Adapter 没有在 sherpa-onnx 结果之外隐式增加标点。

## 推荐标点制品

sherpa-onnx 官方把中英 CT-Transformer 定义为 **offline punctuation**：一次处理
完整输入文本。官方示例把“我们都是木头人不会说话不会动”恢复为“我们都是木头人，
不会说话，不会动。”。Saymore 已固定的 `sherpa-onnx 1.13.4` 也公开了对应 Rust
API，无需引入第二套推理运行时。
[官方模型与示例](https://k2-fsa.github.io/sherpa/onnx/punctuation/pretrained_models.html#sherpa-onnx-punct-ct-transformer-zh-en-vocab272727-2024-04-12-int8)
[1.13.4 Rust 示例](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.4/rust-api-examples/examples/offline_punctuation.rs)

官方发布物身份如下：

| 项目           | 值                                                                           |
| -------------- | ---------------------------------------------------------------------------- |
| Release asset  | `sherpa-onnx-punct-ct-transformer-zh-en-vocab272727-2024-04-12-int8.tar.bz2` |
| 压缩包大小     | 64,717,756 字节（61.72 MiB）                                                 |
| 压缩包 SHA-256 | `c0d5aa5f8eeb686032345e180bedf39319dc2e0556781c6264bcadba8328a6e1`           |
| 运行必需文件   | `model.int8.onnx`                                                            |
| 模型大小       | 75,519,198 字节（72.02 MiB）                                                 |
| 模型 SHA-256   | `65a3fb9f5ad7bfb96bf69e0dc4481df97f6ee60513c1d94ce981ba6effd524b1`           |

压缩包大小和哈希来自
[GitHub Release API](https://api.github.com/repos/k2-fsa/sherpa-onnx/releases/assets/264954276)；
模型哈希由该官方包解压后计算。官方文档说明 sherpa-onnx 运行时只需要
`model.int8.onnx`。

## 离线与实时边界

sherpa-onnx 的 `OnlinePunctuation` 配置使用 CNN-BiLSTM 和 BPE 词表，但当前官方
模型 `sherpa-onnx-online-punct-en-2024-08-06` 明确只支持英语。
[官方在线模型说明](https://k2-fsa.github.io/sherpa/onnx/punctuation/pretrained_models.html#sherpa-onnx-online-punct-en-2024-08-06)
[1.13.4 online Rust 示例](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.4/rust-api-examples/examples/online_punctuation.rs)
运行时存在 online 类型不等于存在可直接用于中文的 online 模型。

FunASR 官方实时听写服务则通过多个模型协同，支持 `online`、`offline` 和 `2pass`；
2pass 先输出实时文字，再在句尾用非实时结果修正并输出标点。服务以独立
`--punc-dir` 加载
`iic/punc_ct-transformer_zh-cn-common-vad_realtime-vocab272727-onnx`。
[FunASR 实时听写官方指南](https://github.com/modelscope/FunASR/blob/main/runtime/docs/SDK_advanced_guide_online_zh.md#funasr实时语音听写服务开发指南)

该 ModelScope 实时标点仓库的 `model_quant.onnx` 是 281,877,652 字节，SHA-256 为
`a167b716ac5ade229b45c0e5c8fafc935cf65abda8b37597c781332ec2970a89`，文件
revision 为 `d6096836258bf2beaf101b4fd0c7dcefb6d95e56`。
[ModelScope 官方模型](https://www.modelscope.cn/models/iic/punc_ct-transformer_zh-cn-common-vad_realtime-vocab272727-onnx)
这条 FunASR CT-Transformer 状态链路不是 sherpa-onnx 当前 CNN-BiLSTM online API
可以直接加载的模型格式，应作为后续独立实验，不能阻塞 Paraformer 首版。

## Saymore 处理顺序

1. 用户选择“本地”且 Paraformer 为当前语音模型时，各加载一次 Paraformer 和
   offline punctuation；切离 Paraformer 或改用 LLM 时释放标点模型。macOS 与
   Windows 使用相同 ONNX 文件与 sherpa-onnx Rust API。
2. partial 只发布 Paraformer 原始文字，不在每个 partial 上运行 offline 标点。
3. 用户松键、停止或端点完成后，先取得 final，再调用一次
   `add_punctuation(final_text)`，随后进入已有最终文本处理与投递。
4. 标点创建或推理失败时保留原始 final；不能把标点失败升级为整次听写失败。
5. 取消会话和空白 final 不运行标点。标点模型使用独立 manifest、下载状态与完整性
   校验；Paraformer 本身的安装完整性不依赖它。只有用户选择“本地”才下载约 61.72
   MiB 的压缩包，安装后只保留 72.02 MiB 的 `model.int8.onnx`。
6. `punctuation_mode` 持久化在 Paraformer provider 配置中。已启用本地模式的用户
   下次启动 Paraformer 时自动加载标点模型；制品缺失时回退到 LLM 模式。

不应对每个 partial 调用 offline CT-Transformer。它每次处理完整字符串，随着 ASR
partial 被修订会重复计算，并造成标点反复跳动；这也不符合 API 的 offline 契约。

首版验收至少覆盖普通话陈述句、问句、短命令、中英混合、数字与小数、缩写、URL、
专名、连续多句、空文本和标点模型损坏回退。分别记录 Apple Silicon、Intel Mac 与
Windows x64 的模型加载时间、峰值内存和 final 增量延迟。官方示例的单次毫秒数据
不能替代 Saymore 目标硬件实测。

## 一手资料

- [FunASR Paraformer 源码](https://github.com/modelscope/FunASR/blob/main/funasr/models/paraformer/model.py)
- [FunASR 实时服务说明](https://github.com/modelscope/FunASR/blob/main/runtime/docs/SDK_advanced_guide_online_zh.md)
- [sherpa-onnx 标点处理契约](https://k2-fsa.github.io/sherpa/onnx/punctuation/index.html)
- [sherpa-onnx 中英 INT8 标点模型与示例](https://k2-fsa.github.io/sherpa/onnx/punctuation/pretrained_models.html#sherpa-onnx-punct-ct-transformer-zh-en-vocab272727-2024-04-12-int8)
- [sherpa-onnx C API 标点文档](https://k2-fsa.github.io/sherpa/onnx/c-api/html/punctuation.html)
