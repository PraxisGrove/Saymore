# SenseVoiceSmall INT8 制品锁定

状态：已锁定并接入 macOS 生产 Adapter 日期：2026-08-04

## 选择

Saymore 使用 sherpa-onnx 发布的 SenseVoiceSmall 五语 INT8 ONNX
转换制品。上游说明该 制品转换自官方
`iic/SenseVoiceSmall`，支持中文、粤语、英语、日语和韩语。运行时固定
`language=auto`、`use_itn=true`，因此最终结果保留 SenseVoice
原生标点和逆文本规范化。它是 FunASR/SenseVoice 生态模型，不是 Qwen3-ASR
家族模型。

模型仓库： `csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17` 固定
revision：`2365baeacb507f821a0c8120fcee3d484dba7a07`

| 本地文件          |      字节数 | SHA-256                                                            |
| ----------------- | ----------: | ------------------------------------------------------------------ |
| `model.int8.onnx` | 239,233,841 | `c71f0ce00bec95b07744e116345e33d8cbbe08cef896382cf907bf4b51a2cd51` |
| `tokens.txt`      |     315,894 | `f449eb28dc567533d7fa59be34e2abca8784f771850c78a47fb731a31429a1dc` |
| `LICENSE`         |          71 | `221c6df10b0931a5629adad671ea48fb7747e034c414b6d2bfa275bc3dd4ea17` |
| `silero_vad.onnx` |     643,854 | `9e2449e1087496d8d4caba907f23e0bd3f78d91fa552479bb9c23ac09cbb1fd6` |

新安装传输总量为 240,193,660 字节，即 229.07 MiB。SenseVoice 文件地址固定到上述
revision；Silero VAD 使用 sherpa-onnx 官方 release asset，并由固定大小和 SHA-256
约束，任何内容漂移都会拒绝激活。

## 运行边界

- 复用 `sherpa-onnx 1.13.4` 的 `OfflineSenseVoiceModelConfig`，不引入 Python
  服务。
- 输入为 16 kHz 单声道 PCM；录音结束后返回 final，不伪造流式 partial。
- 使用 sherpa-onnx 官方 Silero VAD 去除静音并产生语音段；单段最长 30
  秒，长录音按 顺序解码并合并。当前 Saymore 仍由用户开始和结束一次听写，VAD
  不擅自结束录音。
- 下载复用通用安装器的断点续传、暂停、继续、取消、空间检查、逐文件哈希校验、
  原子激活、删除与损坏恢复。

## macOS 验收

在 Apple Silicon 上使用固定仓库的
`test_wavs/zh.wav`，生产安装器完成下载与校验后，生产 Adapter
连续两次均输出“开放时间早上 9 点至下午 5 点。”，证明 `use_itn=true` 的
数字规范化和标点被保留。VAD 段首尾各保留 300 ms 后，首次加载为 0.35 秒，两次
推理均为 0.18 秒；`/usr/bin/time -l` 测得完整探针进程峰值常驻内存 600,915,968
字节，约 573 MiB。无边界 padding 时样本首词会退化，因此该 padding 是验收约束。

## 资料

- sherpa-onnx SenseVoice 预训练模型说明：
  <https://k2-fsa.github.io/sherpa/onnx/sense-voice/pretrained.html>
- SenseVoice 官方仓库：<https://github.com/QwenAudio/SenseVoice>
- 固定转换制品：
  <https://huggingface.co/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/tree/2365baeacb507f821a0c8120fcee3d484dba7a07>
