# Whisper 词典提示与引擎迁移

日期：2026-11-09

## 目标与选择

本地 Whisper 在识别声音时使用个人词典，且无需开启云端或文本润色。旧 sherpa-onnx
1.13.4 Whisper adapter 忽略了 `SpeechRecognitionHints`，原生解码器 没有用户
initial-prompt 路径。采用已有 whisper.cpp 引擎，不维护自有模型或原生解码器分支。

运行时锁定 `whisper-rs = 0.16.0`，Cargo.lock 锁定 `whisper-rs-sys = 0.15.0`，
后者内置 whisper.cpp 1.8.3。当前使用 CPU，保持 4 个推理线程、结束录音后输出、29
秒 顺序分段的契约。构建需要 CMake 和 C/C++ 编译器；`.cargo/config.toml` 设置
`GGML_NATIVE=OFF`，避免按开发机器指令集优化分发包。Windows 保留静态 MSVC CRT。
`whisper-rs` 使用 Unlicense，已纳入依赖许可白名单；whisper.cpp 和 Whisper 模型为
MIT。

上游接口依据：
[whisper-rs 0.16.0 参数 API](https://docs.rs/whisper-rs/0.16.0/whisper_rs/struct.FullParams.html#method.set_tokens)
与
[whisper.cpp 初始提示参数](https://github.com/ggml-org/whisper.cpp/blob/v1.8.3/include/whisper.h)。

## 固定制品

来源：whisper.cpp 官方使用的 Hugging Face 仓库 `ggerganov/whisper.cpp`。固定
revision API 的 LFS 元数据用于核对文件大小和 SHA-256，不将签名 CDN 地址写入
manifest。

| 字段     | 值                                                                 |
| -------- | ------------------------------------------------------------------ |
| Revision | `5359861c739e955e79d9a303bcbc70fb988958b1`                         |
| 文件     | `ggml-large-v3-turbo-q8_0.bin`                                     |
| 字节数   | `874188075`（约 833.69 MiB）                                       |
| SHA-256  | `317eb69c11673c9de1e1f0d459b253999804ec71ac4c23c17ecf5fbe24e259a1` |

[固定下载地址](https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-large-v3-turbo-q8_0.bin)

[固定 revision 元数据](https://huggingface.co/api/models/ggerganov/whisper.cpp/revision/5359861c739e955e79d9a303bcbc70fb988958b1?blobs=true)

继续复用现有下载器的断点续传、大小/哈希校验、校验标记和原子激活。旧 ONNX
encoder/decoder/tokens 不能被 whisper.cpp 使用。新 revision 使用独立安装与
暂存目录，旧文件保留，不会自动删除。旧安装不计为新模型已下载；用户需下载新模型并
重新选择。逻辑 ID `whisper-large-v3-turbo-int8`
保留为历史记录和下载队列的持久化键，不表示新制品仍为 ONNX INT8；展示名称改为
Q8_0。

## 词典契约

- 正常听写和恢复录音继续从共同入口读取词典。每次会话保存自己的规范词条快照。
- 去除空词、含控制字符的词条及大小写不敏感的重复项，保留规范写法。
- 完整词条和逗号分隔符合计最多 192 UTF-8
  字节；超预算词条跳过，允许后面的短词进入。字节级 BPE 的 token
  数不超过字节数，因此留在 Whisper 的 224-token initial-context
  窗口以内。词典过大时不承诺全部注入。
- 用模型 tokenizer 生成 token，通过 `FullParams::set_tokens` 设置原生
  `prompt_tokens`。这与 `initial_prompt` 使用同一初始上下文路径，避免绑定字符串
  setter 的分配及 NUL panic。
- 每个音频分段使用同一词典快照和全新 decoder
  state。模型只加载一次，不保留前次听写 的输出作为后续会话上下文。
- 禁止输出词典、音频、识别内容到原生日志；安装无输出后端的 Whisper/GGML logging
  hooks。
- 提示改善候选上下文，不是强制词表，不保证某个词必然命中。标准音频 UI
  自检仍使用空词典。

词典页仅对 SenseVoice 和 Paraformer 显示识别阶段不支持的提示；Whisper
不再进入此分支。后处理润色是独立功能。

## 验证

确定性回归测试覆盖词典传入每个音频分段、相邻会话隔离、空音频、取消、长音频样本不丢失、
UTF-8
完整词条预算、重复/控制字符过滤和旧模型迁移。词典传递测试先在旧实现得到三个空提示
而失败，再在新实现通过。

生产 adapter 的真实音频探针沿用兼容命令名 `whisper_onnx_probe`，实际使用新 GGML
manifest 和 whisper.cpp。参数第三项用 `-` 跳过精确文字断言，后续参数为词典条目：

```bash
cargo run -p template-infra --example whisper_onnx_probe -- MODELS_ROOT FIXTURE.wav - Saymore 思源黑体
```

2026-11-09 在 macOS Apple Silicon 使用独立测试目录完成实际下载、大小/SHA-256
校验和生产 adapter 识别。将仓库 `standard-zh.pcm` 包装为 16 kHz 单声道 WAV，
加入 `Saymore`、`思源黑体`
两个提示，连续两次识别均输出“可以通过语音进行人机交互”。模型加载 1.44
秒，两次推理分别为 12.50 秒、14.57 秒；测试时机器存在明显内存压力，
这组耗时不作为稳定性能基准，也不是峰值内存测量。随后以空词典再运行两次，
均通过标准句“可以通过语音进行人机交互”的精确输出断言。

该音频不含这两个专有词，结果仅证明新权重、提示路径和连续会话可运行。
词典传入每个音频分段及跨会话隔离由确定性测试覆盖；热词命中率尚未做专门对比。
Windows 编译、打包与干净机器验收未在此 macOS 环境执行。
