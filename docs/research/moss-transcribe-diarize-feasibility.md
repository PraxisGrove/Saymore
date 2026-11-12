# MOSS-Transcribe-Diarize 原生接入可行性

调研日期：2026-11-12。范围：判断
[OpenMOSS-Team/MOSS-Transcribe-Diarize](https://huggingface.co/OpenMOSS-Team/MOSS-Transcribe-Diarize)
（约 0.9B）能否在不依赖 Python 运行时、只用原生推理、不额外部署推理 DLL
的前提下，作为 Saymore 的本地 ASR
接入。本次只做可行性研究，没有修改生产代码，也没有镜像或上传权重。2026-11-11 的
[初筛](local-asr-expansion.md#screenshot-candidates-checked)只看到
Python/PyTorch/CUDA 路径；本文用固定 revision
的上游文件、社区转换制品和本机导出实测补全证据。

## 结论

**判断：需要条件。技术上可以原生集成，不需要 Python。** Saymore 已经链接 ONNX
Runtime 1.27.0，并已通过 `ort = 2.0.0-rc.12` 在 SeACo 上复用它；用它运行本模型
的两段 ONNX 图（Whisper 编码器 + 带 KV cache 的 Qwen3 解码器）不需要新增推理库。
前端（Whisper log-mel）、prompt 拼装、tokenizer、贪心解码循环和输出解析都能用
Rust 实现。但以下条件满足前不应上架：

1. **制品来源。** 上游只发布 BF16 safetensors 和 `trust_remote_code`
   代码，没有官方 ONNX。社区 ONNX 中，`soniqo` FP16 版在 CPU 上的实测 RSS 约 7.4
   GB；它的 INT8 版依赖 WebGPU EP 和 Core ML 编码器，Saymore 链接的 CPU
   运行时用不了。现实路线是 Saymore 从固定上游 revision 自行导出并量化，再像
   SeACo 一样走**导入并校验哈希** 的流程；或者在许可允许的前提下，为第三方 ONNX
   制品锁定 revision 和哈希。
2. **资源预算。** 自导出的动态 INT8 制品为 1.44 GiB。在 M4 CPU、4 线程上，3.9 秒
   中文听写从松键到出全文约 2.3 秒，其中 1.75 秒是固定的 30 秒窗口编码；峰值 RSS
   2.2–2.6 GB。38 秒双说话人音频需 16 秒（RTF
   0.42）。详见[本机实测](#本机实测)。它比 SeACo 和 SenseVoice
   重，短句延迟主要被编码器的固定开销决定。
3. **产品语义。** Saymore 的识别端口只返回一个最终文本字符串。时间戳和说话人标签
   必须在 infra 里解析后丢弃；如果要保留，需要另行设计 app
   层端口，不属于本次范围。热词可以映射到现有 `SpeechRecognitionHints`。
4. **只能离线出结果，没有 partial。** 模型卡和上游代码只描述整段输入（单次最长
   90 分钟），没有提到流式或实时输出。编码器每次都要处理完整的 30 秒窗口，解码器
   要拿到全部音频 token 才开始生成，所以录音过程中拿不到中间结果。Saymore 只能在
   松键后一次性显示全文，与现在的 SeACo、Moonshine 相同。生成阶段可以逐 token
   回调（LiteRT 移植就是这样做的），但这发生在录音结束之后，3.9 秒听写只占约 0.5
   秒，没有产品意义。对增长中的缓冲区反复整段重识别可以模拟 partial，但每次至少
   付出 1.75 秒编码开销，4 线程下做不到每秒刷新，还会和录音争 CPU，不建议。

由于自回归解码和 30 秒窗口，它更适合“会议/长录音转写”或“高精度离线最终稿”，
不适合替代 Zipformer 做实时 partial。建议以“实验性”档位接入，并且排在 Qwen3-ASR
词典提示的真实音频验收之后。

## 1. 模型结构与自定义代码

固定上游 revision：`10304d926740cf5a5cee05a268086e3265197910`（2026-10-31，仅改
README）。权重最后一次变更是 `6ed90e4371`（2026-07-08，“Upload iter 399 …
checkpoint”），之后的提交都只改文档或配置声明，所以各社区转换使用的是同一份权重。

| 组成       | 结构                                                                                       | 参数量（soniqo 统计）   |
| ---------- | ------------------------------------------------------------------------------------------ | ----------------------- |
| 音频编码器 | Hugging Face `WhisperEncoder`：80 mel、24 层、d_model 1024、16 头、1500 位置               | 312,463,360（含适配器） |
| 适配器     | 4 帧拼接（1024→4096），`Linear→SiLU→Linear→LayerNorm` 投影到 1024                          | 同上                    |
| 文本解码器 | Hugging Face `Qwen3Model`：28 层、hidden 1024、16 Q 头 / 8 KV 头、head_dim 128、RoPE θ=1e6 | 596,049,920             |
| 输出       | `lm_head` 与词嵌入绑定，词表 151,936                                                       | —                       |

数据流：16 kHz 单声道 → 每 30 秒一块，补零 → Whisper log-mel（80×3000）→ 编码器
1500 帧 → 截到 `4 × ceil(samples/1280)` 帧 → 4 帧合并 → 适配器得到每 80 ms 一个
音频 token（12.5 token/s）→ 按顺序填进 prompt 里的 `<|audio_pad|>` 位置 → Qwen3
贪心生成，遇到 `<|im_end|>` 停止。

**`trust_remote_code`：需要，但内容很薄。** 模型卡要求
`trust_remote_code=True`，`config.json` 的 `auto_map` 指向三份自定义文件：
`configuration_…`（2.6 KB）、`modeling_…`（15 KB）、`processing_…`（11
KB）。审阅后确认它们只组合 Transformers 自带的 `WhisperEncoder` 和
`Qwen3Model`，加上上述两层适配器、`masked_scatter` 注入和处理器逻辑，没有自定义
CUDA 或算子。本次实测**没有执行任何下载的仓库代码**：探针脚本用 Transformers
原生类构建同一结构，直接加载
safetensors。逐键加载严格匹配，没有缺失或多余的权重。

处理器里有一处容易漏掉的约定：音频 token 段中**每 5 秒插入一次十进制数字 token**
（`processor_config.json`：`time_marker_every_seconds: 5`，
`audio_tokens_per_second: 12.5`，每 `int(62.5)=62` 个音频 token 后插入
`5`、`10`…）。这些数字是普通文本 token，不接收音频嵌入。小于 5
秒的听写不会插入数字，但 Rust 实现必须逐位复刻，否则长音频的时间戳会漂移。

`tokenizer_config.json` 的 `tokenizer_class` 是无意义字符串 `Qwen2Tokenizer`，
`AutoTokenizer` 不能依赖它；`tokenizer.json` 本身是标准 Qwen2 字节级 BPE（NFC
normalizer + Qwen 分词正则 + ByteLevel），音频特殊 token 为 151669–151671。

## 2. 许可

- 模型卡 YAML 与正文都写明 **Apache-2.0**；上游 GitHub 仓库
  [OpenMOSS/MOSS-Transcribe-Diarize](https://github.com/OpenMOSS/MOSS-Transcribe-Diarize)
  附有 Apache-2.0 全文。Hugging Face 模型仓库本身**没有 LICENSE 文件**（固定
  revision 下请求 `LICENSE` 返回 404）。
- Apache-2.0
  允许终端用户从上游下载使用，也允许再分发转换后的制品，但需保留许可和
  NOTICE。Saymore 不镜像权重，因此直接引导用户从上游下载不存在许可障碍。
- 社区转换（soniqo ONNX、handy-computer / cstr GGUF）都声明 Apache-2.0
  并指向上游。若改用第三方制品，需在制品锁定文档里另行记录来源和
  revision；转换者不是模型提供方。
- 上游没有公布训练数据清单；cstr
  的转换卡也指出了这一空缺。这不影响许可，但发布前
  应像其他模型一样，在产品文案里标明来源。

## 3. 原生运行路径

| 路线                                                                                                                                                             | 状态                                                                                                                                                                                                                                                                                   | 对 Saymore 的结论                                                                                                                                             |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| sherpa-onnx                                                                                                                                                      | 截至 v2.0.1（2026-10-26）没有 MOSS 模型类型；只支持结构相近但不同的 Qwen3-ASR 和 FunASR-Nano。                                                                                                                                                                                         | 不能直接配置。Qwen3-ASR 的 C++ 实现也不能复用：编码器、mel 维度和 prompt 都不同。                                                                             |
| 自行导出 ONNX（本次实测）                                                                                                                                        | 两段图：`audio_encoder.onnx`（`input_features [1,80,3000]` → `audio_embeds [1,375,1024]`）和 `decoder.onnx`（`input_ids`、`audio_embeds`、`audio_mask`、`past_keys/values [28,1,8,P,128]` → `logits`、新增 K/V）。opset 18。                                                           | **推荐。** 复用已链接的 ORT 1.27.0 CPU（静态库同时含 `MatMulNBits`、`GroupQueryAttention` CPU 内核），不新增 DLL。                                            |
| [soniqo ONNX FP16](https://huggingface.co/soniqo/MOSS-Transcribe-Diarize-0.9B-ONNX-FP16)（`3110ffbd`）                                                           | 同样的两段式契约，文档完整，含哈希；M5 Pro CPU：RTF 0.28，24 ms/token，RSS 约 7.4 GB。                                                                                                                                                                                                 | 可作为契约参考和对照；内存过高，不适合作为默认制品。                                                                                                          |
| [soniqo ONNX INT8](https://huggingface.co/soniqo/MOSS-Transcribe-Diarize-0.9B-ONNX-INT8)（`45b3b835`）                                                           | Core ML 编码器 + WebGPU `MatMulNBits` 解码器，KV cache 固定 1024 token；RTF 0.046，RSS 约 2.2–2.6 GB。                                                                                                                                                                                 | 依赖 WebGPU EP 预览插件和 Core ML，与“只用已链接 CPU 运行时、不加 DLL”冲突；也不跨平台。                                                                      |
| GGUF：[transcribe.cpp](https://huggingface.co/handy-computer/moss-transcribe-diarize-gguf)、[CrispASR](https://huggingface.co/cstr/MOSS-Transcribe-Diarize-GGUF) | Q8_0 987 MB；卡片称 M4 CPU 3.66× 实时、Metal 9.42×；CrispASR 支持 `--hotwords`。                                                                                                                                                                                                       | 原生但需新增一个 C++ 推理库（ggml）。Saymore 已经静态链接 whisper.cpp 的 ggml，再链接一份很可能出现重复符号，或要求两边 ggml 版本对齐。不推荐作为第一条路线。 |
| MLX / Core ML（多家）                                                                                                                                            | 仅 Apple 平台。                                                                                                                                                                                                                                                                        | 不符合跨平台和单一运行时约束。                                                                                                                                |
| LiteRT / TFLite：[Luigi/moss-transcribe-diarize-litert](https://huggingface.co/Luigi/moss-transcribe-diarize-litert)（`d900e517`），以及同名 Space               | 三段 flatbuffer（编码器、绑定的嵌入/lm_head、外部 KV 的解码器），有 f32/fp16/q8/int4 几版。卡片称 f32 与 PyTorch 参考逐字一致，int4 C++ 引擎峰值 RSS 1.51 GB。Space 实际运行的 v2 int4 解码器经过作者自己的静音鲁棒 QAT 训练，不是上游原权重。Space 用 Python `ai-edge-litert` 2.1.6。 | 需要新增 LiteRT/TFLite 原生库（XNNPACK），违反“不加推理 DLL”；v2 解码器也不是上游权重。不采用，但它的静音测试结论值得参考（见第 8 节）。                      |
| OpenVINO IR：[johnpattern/safenotes-moss-hq](https://huggingface.co/johnpattern/safenotes-moss-hq)（`5a00a83d`）                                                 | 编码器 fp16 + 有状态 LM int8 + OpenVINO tokenizer IR，卡片列出哈希。                                                                                                                                                                                                                   | 需要 OpenVINO 运行时，偏向 Intel 硬件。不采用。                                                                                                               |
| OpenASR `.oasr`：[OpenASR/moss-transcribe-diarize](https://huggingface.co/OpenASR/moss-transcribe-diarize)（`196b6d49`）                                         | 自有打包格式，fp16/q8_0/q4_k，需要安装 OpenASR CLI 和运行时；卡片自报中文 CER 2.52%。                                                                                                                                                                                                  | 新运行时加非标准格式。不采用。                                                                                                                                |

2026-11-12 用 Hugging Face API 查询了模型树。`base_model:quantized:` 下的 16
个量化版本是 8 个 GGUF（cstr、mudler、handy-computer、Luigi
zh-TW、voicetide、myflow-ai、tryambientai、mrom1）、6 个 MLX 和 2 个
ONNX（即上表 soniqo 两版）。LiteRT、4 个 Core ML、OpenVINO 和 OpenASR 被标为
finetune，也逐一看过。除 soniqo FP16 外，没有一个能直接跑在 Saymore 已链接的 CPU
ORT 上。第三方制品一律按不可信数据处理：本次只读了
卡片和文件列表，没有下载这些权重，也没有运行 Space 的代码。

## 4. 前端、tokenizer 与解码循环的 Rust 实现

全部可以在 `crates/infra` 内用 Rust 完成，不需要 Python：

- **Whisper log-mel：** 400 点周期 Hann 窗、hop 160、`n_fft` 400、功率谱、80 维
  Slaney mel 滤波器（可在构建时由公式生成，或随制品附带 `mel_filters`
  常量）、`log10`、 `max(x, max−8)`、`(x+4)/4`。每块先补零到 480,000
  采样，归一化最大值包含补零部分，这与上游一致。`realfft 3.5.1` 已经作为
  `kaldi-fbank` 的传递依赖出现在 `Cargo.lock`，可以直接提升为 workspace
  依赖。需用 Python 参考特征做一次数值对齐
  测试（离线生成的定值样例，不在运行时依赖 Python）。
- **Tokenizer：** 运行时只需要 (a) 把固定 prompt 和热词编码成 BPE id，(b)
  把生成的 id 解码回 UTF-8。解码只要查 `vocab.json`
  加字节级反映射，很简单。编码需要 Qwen2 分词正则（含 `\s+(?!\S)` 前瞻）+ BPE
  merges。两种做法：引入 `tokenizers` crate（关闭默认特性，启用
  `fancy-regex`），与上游逐位一致；或自写约 200 行的分词与 BPE。
  前者更稳妥，但是新依赖，需要在变更说明里写明理由。
- **Prompt：** 固定模板为
  `<|im_start|>system\nYou are a helpful assistant.<|im_end|>\n<|im_start|>user\n<|audio_start|>{音频 token 与时间数字}<|audio_end|>\n{指令}{热词}<|im_end|>\n<|im_start|>assistant\n`。
  不需要实现 Jinja；指令和热词以外的 id 可以在加载时编码一次并缓存。
- **解码循环：** 一次 prefill（`audio_mask=1`
  的位置用音频嵌入），之后每步输入一个 token，把新 K/V 追加到 `P` 维；贪心
  argmax，遇到 151645/151643 停止，并设 `max_new_tokens` 上限和取消检查。解码器
  FP32 下每 token 的 KV 是 28×2×8×128×4 B = 224 KiB，3 分钟听写约 2,400
  token、约 0.5 GiB，需要设长度上限。
- **输出解析：** `[start][Sxx]text[end]` 串接。上游提供无正则的流式状态机
  `TranscriptStreamParser`，可以等价移植；对 Saymore 而言只需按序拼接
  `text`，并对
  畸形片段做保守处理（只去掉能识别的时间戳和说话人标记，其余原样保留）。
- **长音频：** 上游处理器把整段音频切成**不重叠**的 30
  秒块，各自编码后拼接成一个 prompt，一次生成全文。按键听写一般在 30
  秒内，单块即可；更长时可以沿用上游的多块拼接，不必像 SeACo/Moonshine
  那样分段独立解码。

## 5. 热词、时间戳、说话人分离

| 能力       | 输入                                                                                                          | 输出                                                   | 映射到 Saymore                                                                                                                                                                                                               |
| ---------- | ------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 热词       | 纯文本 prompt：在默认中文指令后追加 `热词提示：词1, 词2, 词3`（英文版为 `Hotwords: …`）。没有专门的偏置模块。 | 影响生成文本，不保证拼写。                             | **可以映射到 `SpeechRecognitionHints`**，每次会话拼进 prompt，不需重载模型。参照 Qwen3 提示的做法：去控制字符、去重、限制字节数（建议 ≤256 UTF-8 字节、≤64 项），并去掉 `[`、`]`、换行和尖括号，防止拼出 prompt 特殊 token。 |
| 时间戳     | 默认指令要求输出段级时间戳；音频段内的 5 秒数字锚点辅助定位。                                                 | `[0.39]…[2.99]`，单位秒，段级（非词级）。              | 现有端口 `finish() -> String` 没有位置，解析后丢弃。若以后做“长录音/会议”功能，需要新增 app 层类型（段落、开始/结束时间），不应塞进字符串。                                                                                  |
| 说话人分离 | 默认指令要求 `[S01]`、`[S02]` 标签；“只要说话人”的指令另有一版。                                              | 每段一个匿名标签，只在本次输入内有效；可能漏判或重叠。 | 同上，听写场景丢弃。注意：多块长音频在同一次生成中保持标签一致，但两次独立会话之间不一致。                                                                                                                                   |

听写场景可以考虑换成不要求时间戳的指令（例如上游的“speaker-only”指令，或自定义
“直接转写为文本”），以减少生成 token
数；但这偏离了上游默认和评测配置，必须单独做
准确率回归后才能采用，本次没有验证。

## 本机实测

环境：Apple M4（10 核）、16 GB 内存、macOS 15.5（Darwin 24.5.0）。Python 3.12
隔离 venv 放在 `target/moss-research/venv`，包括 torch 2.15.0、transformers
5.22.2、 onnx 1.23.2、onnxruntime **1.27.0**（与 sherpa-onnx 1.13.4 静态链接的
ORT 版本相同，可作为 Rust `ort` 运行效果的代理）。CPU EP，4 个 intra-op 线程（与
SeACo 相同），
`ORT_ENABLE_ALL`，贪心解码。时间不含模型加载；表中是同一进程内的最后一次
（预热后）运行。峰值 RSS 取 `/usr/bin/time -l` 的进程最大驻留集。

**测量说明：** 测试期间，本机有一个 Virtualization.framework 虚拟机（约 8 GB
驻留、15 GB 压缩）和其他会话的 `rustc` 构建，swap 一度用到 22
GB。早期运行的缺页数达数十万，延迟因此失真，未采用。下表 INT8 一行取自缺页数约
200 次的安静时段；NBits
两行测量时仍有数万次缺页，延迟只作量级参考。这些数据都不是空闲机器上的基准。

### 精度与体积

| 制品                                  | 量化                                             | 运行文件总大小              | 正确性（`standard-zh`）                             |
| ------------------------------------- | ------------------------------------------------ | --------------------------- | --------------------------------------------------- |
| 上游 safetensors                      | BF16                                             | 1,817,113,576 B             | —                                                   |
| 自导出 FP32 ONNX                      | 无                                               | 4,257,063,814 B（3.96 GiB） | 解码器与 Qwen3Model logits 余弦 1.0000，argmax 一致 |
| 自导出 **动态 INT8**（MatMulInteger） | 权重 INT8、激活动态量化；词嵌入 Gather 仍为 FP32 | 1,548,080,424 B（1.44 GiB） | 与 PyTorch FP32 输出逐字一致                        |
| 自导出 NBits4（`MatMulNBits`，块 32） | 4-bit 对称                                       | 1,208,819,357 B（1.13 GiB） | 文字一致；时间戳 `[0.36]…[2.98]`，略有漂移          |
| 自导出 NBits8（`MatMulNBits`，块 32） | 8-bit 对称                                       | 1,660,428,232 B（1.55 GiB） | 与 FP32 逐字一致                                    |

运行时还需要 `vocab.json`（2,776,833 B）与 `merges.txt`（1,671,853 B），或者
`tokenizer.json`（11,423,222 B）。上表中 FP32 的 622 MB 词嵌入未量化；改用 INT8
`GatherBlockQuantized` 预计可再省约 0.45 GB，但本次没有验证。

### 延迟与内存（动态 INT8，安静时段）

| 用例                                    | 音频 (s) | prompt / 生成 token | 编码器 (s) | prefill (s) | 解码 (ms/token) | 识别总计 (s) |   RTF | 峰值 RSS |
| --------------------------------------- | -------: | ------------------: | ---------: | ----------: | --------------: | -----------: | ----: | -------: |
| `standard-zh.pcm`                       |    3.859 |            135 / 23 |       1.76 |        0.11 |            18.4 |     **2.29** | 0.594 |  2.24 GB |
| 同上 + 热词 `人机交互,语音识别,Saymore` |    3.859 |            149 / 23 |       1.83 |        0.11 |            17.6 |         2.35 | 0.609 |  2.59 GB |
| `standard-en.pcm`                       |    2.737 |            121 / 25 |       1.74 |        0.09 |            16.0 |         2.24 | 0.817 |  2.54 GB |
| 中/英/中拼接，两个说话人                |   11.455 |            233 / 68 |       2.82 |        0.22 |            23.8 |         4.66 | 0.407 |  2.58 GB |
| 5 × (中 + 英) 拼接，两个 30 s 块        |   37.980 |           574 / 241 |       4.56 |        0.75 |            44.4 |        16.01 | 0.422 |  2.75 GB |

- 冷启动：加载两段会话 1.1–1.6 s，加载后 RSS 约 1.44 GB；加载后第一次识别
  `standard-zh` 需 3.2–3.9 s，此后约 2.3 s。
- **固定开销主要在编码器**：哪怕只有 1 秒语音，也要编码完整的 30 秒窗口（约 1.75
  s）。这是 Whisper
  结构决定的。截短编码器输入可以降低延迟，但会偏离训练分布，需要另做
  一致性验证，本次没有尝试。
- 8 线程没有帮助（`standard-zh` 合计 2.37 s，解码 25 ms/token）。
- 长音频的 ms/token 变大，一是 KV 变长，二是探针在 Python 里每步拼接 KV
  数组。Rust 实现可以预分配 KV 缓冲区，避免这部分拷贝。
- 对照：PyTorch FP32 参考运行（同机、4 线程，在高负载时段）RSS 3.40 GB。

**内核选择很关键。** 同一导出，用 `MatMulNBits` 量化后：NBits8
在两次复测中解码都是 348–376 ms/token（`standard-zh` 合计 10.9–11.9 s）；NBits4
是 26 ms/token、编码器 3.2 s（合计 4.29 s，峰值 RSS 1.89 GB）。动态 INT8 走 ARM
上成熟的 `MatMulInteger` 路径，是本机最快的组合；NBits4
体积和内存最小。量化格式要在 Windows x64 上再比较一次。

### 功能观察

- `standard-zh`：`[0.39][S01]可以通过语音进行人机交互。[2.99]`，与 AISHELL
  参考文本一致（含句号）。
- `standard-en`：`[0.07][S01] The quick brown fox jumps over the lazy dog.[1.81]`。
  注意文本前有一个空格，解析时需要 trim。
- 中/英/中拼接：`[0.53][S01]…[3.31][4.28][S02]The quick brown fox…[7.11][8.11][S01]…[10.91]`。
  说话人分离正确，并且第三段重新认出了 S01；时间戳与 0.5 s 静音间隔吻合。
- 38 秒双块：10 段全部正确，跨越 30 秒块边界后 S01/S02 仍然交替一致，时间戳以约
  3.8 s 为周期递增，说明 5 秒数字锚点和多块拼接的实现正确。
- 加热词后文本不变，结束时间戳从 2.99 变成 3.31。这只说明热词不会破坏识别，不能
  说明它有纠错效果。

### 第三方参考数据（非本机）

- soniqo FP16 ONNX，CPU EP，Apple M5 Pro：英文 80 条 RTF 0.28，24 ms/token，RSS
  约 7.4 GB。
- soniqo INT8（Core ML + WebGPU），M5 Pro：RTF 0.046，RSS 2.2–2.6 GB。
- transcribe.cpp Q8_0（987 MB），卡片自报 M4 CPU 3.66× 实时、Metal 9.42×；FLEURS
  中文 CER 9.23 (Q8_0).

## 6. 接入 `crates/infra` 的方案草图

只新增 infra 适配器，沿用 `StreamingSpeechRecognizer` 端口，不改依赖方向
（`desktop → app`、`desktop → infra → app`）。app
层除了新模型身份、能力声明和存储统计以外不需要新类型。

```text
crates/infra/src/
  ort_runtime.rs                 # 从 seaco_asr/runtime/initialization.rs 提升：
                                 # 进程内一次 OrtGetApiBase + ort::init，SeACo 与 MOSS 共用
  moss_asr.rs                    # MossSpeechRecognizer / MossSession（缓冲 PCM，finish 时解码）
  moss_asr/
    model.rs                     # ModelFiles::validate：两段 ONNX + 外部数据 + tokenizer 文件
    frontend.rs                  # Whisper log-mel（realfft）；30 s 块补零
    prompt.rs                    # 固定模板 id 缓存、音频 token 与 5 s 数字锚点、热词拼接
    tokenizer.rs                 # BPE 编码/字节级解码（tokenizers crate 或自写）
    runtime.rs                   # 两个 ort::Session；prefill + KV 追加 + 贪心循环；取消检查
    transcript.rs                # [t][Sxx]text[t] 流式解析 → 纯文本
    hotwords.rs                  # 清洗、去重、字节上限；去掉 [ ] < | 和控制字符
    tests.rs
  model_installer/moss_manifest.rs  # 固定 revision、大小、SHA-256
```

要点：

- **会话语义**与 SeACo 一致：`start` 时快照热词；`push_audio` 只缓冲；`finish`
  时一次编码全部 30 s 块，单个 prompt 生成；`cancel`
  设置原子标志，解码循环每步检查。不产生 partial，不伪造流式。
- **共享模型**：两个 `Session` 放在 `Mutex` 后面（ORT 会话可以并发运行，但 KV
  内存会翻倍，先串行化更稳妥）。KV cache 归会话所有，不留在共享状态里。
- **制品格式**：首选动态 INT8（`MatMulInteger`），本机实测最快；如果内存比延迟更
  重要，备选 NBits4。不要用本机实测很慢的 NBits8。KV
  用每层预分配的缓冲区并按长度切片，不要每步重新拼接数组。
- **契约检查**：加载时校验输入输出名、dtype、维度（`[1,80,3000]→[1,375,1024]`、
  `past_keys [28,1,8,P,128]`、`logits [1,151936]`），并检查 logits
  全部有限，沿用 SeACo 的 `check_contract` 写法。
- **边界**：`max_new_tokens` 按音频时长设上限（例如
  `12.5 × 秒数 + 64`，再加绝对上限），
  防止重复生成跑飞；输出为空或只有标记时返回 `Protocol` 错误，而不是空串。
- **安装**：与 SeACo 相同，提供**导入经校验的导出目录**，manifest
  固定每个文件的大小和哈希，不提供下载按钮。导出脚本与参考特征样例放在
  `crates/xtask` 或文档里作为维护
  工具，不进入运行时。如果以后要做下载，需要一个可信、固定 revision 的 CPU ONNX
  发布源。
- **激活自检**：沿用现有“加载 → `standard-zh.pcm` 识别 →
  成功后才保存选择”的流程。期望文本（去掉标记后）是
  `可以通过语音进行人机交互。`。

## 7. 工作量估计

按一名熟悉本仓库的工程师估算，不含真实音频准确率评测的数据准备：

| 工作项                                                               | 估计             |
| -------------------------------------------------------------------- | ---------------- |
| 导出脚本固化（编码器、KV 解码器、量化、嵌入量化）+ 参考特征/输出样例 | 1–1.5 天         |
| `ort_runtime` 提升（SeACo 回归不变）                                 | 0.5 天           |
| Whisper log-mel + 数值对齐测试                                       | 1 天             |
| Tokenizer（编码/解码）+ prompt/时间锚点 + 对齐测试                   | 1–1.5 天         |
| ORT 运行时、贪心循环、取消、契约检查、转写解析                       | 1.5–2 天         |
| 安装 manifest、导入、存储统计、模型卡与 UI 文案、激活自检            | 1.5–2 天         |
| macOS 与 Windows 原生验收、内存/延迟测量、文档                       | 1–2 天           |
| **合计**                                                             | **约 8–11 人日** |

另需一次真实听写集评测（普通话、中英混说、专名、噪声），与 Qwen3-ASR、SeACo
用同一切分和文本规范化对比，然后才能决定它是否值得作为一个档位上架。

## 8. 风险与未验证项

- **准确率没有验证。** 本次只用仓库自带的一条 AISHELL 样例、合成英文样例和它们的
  拼接确认链路正确，不构成 CER/WER 对比。上游评测在会议和播客语料上，用 BF16
  推理；INT8 导出的精度损失必须在 Saymore 的听写集上成对测量。
- **热词效果没有验证。** 本次只确认加入热词后识别文本不被破坏（结束时间戳从 2.99
  变成 3.31）；没有找到能观察到纠正效果的公开样例。热词靠 prompt
  生效，不保证拼写，
  也可能诱导模型在无关位置输出热词，需要用真实专名样例做正反两向测试。
- **自回归失控。** LLM 解码可能重复、漏句或在静音上幻觉，需要 token 上限、空输出
  判错，并测试纯静音和噪声样例。soniqo
  卡片也指出时间戳可能畸形或重叠、说话人可能漏判。LiteRT 移植的卡片报告，未做
  QAT 的 int4 解码器在静音尾段会循环输出 1,185 字符的英文幻觉；本次的动态 INT8
  没有测静音，必须补上。上游还声称能输出声学事件标注，本次样例没有出现，格式
  未知，解析器要对无法识别的片段保守处理。
- **延迟没有在空闲机器上测到。** 见[本机实测](#本机实测)的说明。
- **Windows 未验证。** 静态 MT 版 ORT 同样带 CPU `MatMulNBits`
  内核，但本次没有在 Windows 上运行。
- **旧 SenseVoiceSmall 与英文测试音频已移除。** 下文保留当时的 MOSS 实测证据，
  不把已下线的本地比较对象或旧 fixture 描述为当前可用功能。
- **制品分发。** 自行导出的文件只能走导入流程；不镜像权重是硬约束。若要提供下载
  按钮，只能依赖第三方固定 revision 的 ONNX，而现有第三方 CPU
  制品体积和内存都偏大。
- **GGUF 路线的链接冲突**（如果改走 transcribe.cpp/CrispASR）：与 whisper.cpp
  共存时 ggml 重复符号的问题没有验证，属于推断风险。

## 复现

所有文件都在被忽略的 `target/moss-research/` 下：`upstream/` 存放固定 revision
的下载（不可信数据，不执行其中代码），`scripts/` 存放探针脚本，`venv/` 是隔离的
Python 环境，`onnx-*` 是导出物，`results-*.jsonl` 是原始测量记录。权重的 SHA-256
`9a0ceb4ab7330357db3ff583dba8d83625d5b733b00e1d55d6970e11b07026c4` 与 Hugging
Face LFS 元数据一致。

```bash
R=10304d926740cf5a5cee05a268086e3265197910
D=target/moss-research
curl -fL -o $D/upstream/model-00000-of-00001.safetensors \
  "https://huggingface.co/OpenMOSS-Team/MOSS-Transcribe-Diarize/resolve/$R/model-00000-of-00001.safetensors"
# config.json、tokenizer.json、vocab.json、merges.txt 同理
/opt/homebrew/bin/python3.12 -m venv $D/venv
$D/venv/bin/python -m pip install torch transformers safetensors numpy onnx onnx_ir "onnxruntime==1.27.0" tokenizers
cd $D
venv/bin/python -I scripts/moss_probe.py reference upstream ../../apps/desktop/assets/asr-test/standard-zh.pcm
venv/bin/python -I scripts/moss_probe.py export upstream onnx-fp32
venv/bin/python -I scripts/moss_probe.py quantize onnx-fp32 onnx-int8dyn int8dyn
venv/bin/python -I scripts/moss_probe.py ort upstream onnx-int8dyn \
  ../../apps/desktop/assets/asr-test/standard-zh.pcm --repeat 3
zsh scripts/run_suite.sh "$PWD" onnx-int8dyn int8dyn
```

探针脚本用 Transformers 自带的 `WhisperEncoder`/`Qwen3Model`
构建模型，再严格加载权重，不使用 `trust_remote_code`。解码器导出是手写的显式 KV
版本，导出前会与 `Qwen3Model` 做 logits 一致性检查。双说话人和 38
秒样例由仓库自带的两条 PCM 拼接
而成，不含用户录音。测试没有读取或修改用户设置和模型目录。
