# 主流云端 ASR 返回结构与元数据能力对比

> 调研日期：2026-07-15。范围是厂商官方公开 API 文档，不代表所有地区、套餐、模型版本和私有化产品。

## 结论

主流云端 ASR **不会统一返回全部元数据**。最常见的是整段文本和服务端切分的片段；词时间戳、说话人标签通常需要显式开启，或仅文件转写支持。音量、语速、情绪主要出现在客服录音、通话分析、发音评测等专项产品；性别几乎不属于标准 ASR 输出。

这些元数据本身不会提高识别准确率。它们主要帮助字幕对齐、说话人排版、质量诊断和后处理。真正可能改变识别结果的是模型/资源版本、热词和上下文、语言提示、ITN/标点/顺滑、二遍识别、VAD 以及音频质量等请求侧能力。

## 这些字段是什么结构

不同厂商命名不同，但常见层级可以抽象成：

```json
{
  "text": "整段识别文本",
  "segments": [
    {
      "text": "服务端切出的连续语音片段",
      "start_ms": 120,
      "end_ms": 1680,
      "speaker": "spk_0",
      "language": "zh-CN",
      "confidence": 0.94,
      "speech_rate": 5.2,
      "emotion": "neutral",
      "words": [
        {
          "text": "你好",
          "start_ms": 120,
          "end_ms": 430,
          "confidence": 0.97,
          "speaker": "spk_0"
        }
      ]
    }
  ]
}
```

- **片段/分句**：`utterance`、`segment`、`result`、`phrase`、`sentence` 都表示服务端切出的连续语音片段，并不是跨厂商统一标准，也不保证等于语法完整的一句话。
- **词时间戳**：每个字或词的开始/结束时间，用于字幕、逐词高亮和从原音频回放定位。
- **说话人分离**：回答“哪个匿名说话人在何时说话”，通常只给 `spk_0`、`speaker=1` 等标签；它不是身份识别，也不是性别识别。
- **检测语言**：可能是模型自动判断的语种，也可能只是回显请求中指定的语言，必须按厂商文档区分。
- **置信度**：可能位于片段、候选或词级；不同厂商的分数不可直接横向比较，缺失也不等于低置信度。
- **音量/语速/情绪**：通常是片段级声学或分析结果。情绪标签也可能来自转写后的文本情感分析，而非声音本身。
- **音频事件**：VAD 的“开始/停止说话”只是边界事件，不等同于笑声、掌声、音乐等通用声音事件分类。

## 能力矩阵

图例：**标配** = 对应 API 常规结构；**可选** = 需开参数；**限定** = 仅特定模型、模式或专项产品；**无** = 官方标准响应未记录该字段。

| 厂商/产品 | 片段结构 | 词时间戳 | 说话人 | 检测语言 | 置信度 | 音量/语速/情绪/性别及事件 |
|---|---|---|---|---|---|---|
| 火山引擎豆包 ASR | 标配/可选 `utterances[]` | `utterances[].words[]` | 限定：录音文件可开启 `speaker` | 限定：标准录音文件有 LID 客服能力 | 词级结构可含 `confidence` | 限定：标准录音文件有音量、语速、情绪、性别开关；极速版明确移除这些客服能力 |
| 阿里云百炼 Paraformer / Qwen3-ASR | 标配 `sentence` 或实时 item | Paraformer 默认有；Qwen 实时无，文件模型有 | 限定：Paraformer 文件转写可开启 | Qwen 实时返回 `language`；Paraformer 可自动识别/提示语种，但实时结果未统一给语言字段 | Paraformer 8k 情绪有 `emo_confidence`；不是通用 ASR 文本置信度 | Qwen 实时返回七类情绪；仅 Paraformer 8k v2 有三类情绪；无标准音量、语速、性别 |
| 腾讯云 ASR | 标配 `ResultDetail[]` / `sentence_list[]` | 标配词列表 | 可选说话人分离，或双声道标识 | 限定 `LangType` | 部分接口/词候选有分数，录音文件 `SentenceDetail` 不以通用置信度为核心字段 | 录音文件可有 `SpeechSpeed`、`EmotionalEnergy`、`EmotionType`；无标准性别 |
| 百度智能云 ASR | 实时按 `MID_TEXT` / `FIN_TEXT` 分句 | 无标准词级时间戳 | 无标准字段 | 无自动检测结果字段；通过模型/PID 选择 | 短语音标准响应无置信度 | 无标准音量、语速、情绪、性别或音频事件字段 |
| 讯飞开放平台 ASR | 流式 `result.ws[]`；文件转写 `lattice[]` | 可返回字词及时间位置 | 文件转写可选角色分离 | 限定：自动多语种模式可在词结果返回 `lg` | 文件转写有片段/词分数；流式文档把 `sc` 标为保留字段 | 标准接口无音量、语速、情绪、性别或音频事件字段 |
| Google Cloud Speech-to-Text V2 | 标配 `results[]`，每项含 `alternatives[]` | 可选 `enableWordTimeOffsets` | 可选 diarization，词上 `speakerLabel` | `result.languageCode`；自动语言选择有模型/区域限制 | 候选级；词级需 `enableWordConfidence`，且不保证总返回 | 无标准音量、语速、情绪、性别；流式 VAD 只给语音活动边界 |
| Azure AI Speech | 实时 utterance；Fast/Batch 为 `phrases[]` | 可选 | 可选 diarization，片段带 `speaker` | Fast/Batch 可带 `locale`；自动语言识别需配置 | Detailed/N-best 或片段级 | 基础 STT 无；发音评测可给流利度/韵律，通话中心情感需组合 Language 服务；无标准性别 |
| Amazon Transcribe | Batch `transcripts[]` + `items[]`；流式 `Results[]` | 发音 item 含开始/结束时间 | 可选 `speaker_labels` | 自动语言识别后返回语言及 score | item 候选级 | 基础版无；Call Analytics 可给响度、WPM、打断、静音/说话时长和轮次情感；无性别 |
| OpenAI Audio Transcriptions | GPT 默认仅 `text`；Whisper `verbose_json` 有 `segments[]`；diarize 模型有带 speaker 的 `segments[]` | 仅 `whisper-1` 的 `verbose_json` 可选 | 仅 `gpt-4o-transcribe-diarize`；该模式不支持词时间戳 | `whisper-1 verbose_json` 有 `language`；普通 GPT JSON 未文档化检测语言字段 | GPT 可选 token `logprobs`；Whisper 片段有 `avg_logprob`，不是统一 0-1 置信度 | 无音量、语速、情绪、性别；VAD 用于切块，不返回音量值 |

## 厂商依据与限制

### 火山引擎豆包 ASR

大模型录音文件接口的 `result.utterances[]` 包含片段文本和起止时间，`words[]` 包含词文本、起止时间和 `confidence`。老版标准录音文件还可通过 `with_speaker_info` 返回片段 `additions.speaker`。[大模型录音文件识别 API](https://www.volcengine.com/docs/6561/1354868) [录音文件识别标准版](https://www.volcengine.com/docs/6561/80820?lang=en)

音量、语速、情绪、性别和语种检测并非当前所有豆包 ASR 接口共有。大模型录音文件极速版明确说明移除了标准版的 `enable_lid`、`enable_emotion_detection`、`enable_gender_detection`、`show_volume`、`show_speech_rate` 等客服能力，因此 Saymore 使用的流式接口不能因为同属豆包就假定会返回这些字段。[大模型录音文件极速版](https://www.volcengine.com/docs/6561/1631584?lang=zh)

流式二遍识别的 `enable_nonstream` 是影响最终识别路径的请求参数，而不是返回元数据：开启后，同一接口先实时返回流式逐字结果，再对 VAD 切出的分句音频做一次非流式重识别。[大模型流式语音识别 API](https://www.volcengine.com/docs/6561/1354869)

### 阿里云百炼 Paraformer / Qwen3-ASR

Paraformer 实时响应按 `sentence` 返回 `begin_time`、`end_time`、`text` 和 `words[]`；词对象含开始/结束时间、文本与标点。只有 `paraformer-realtime-8k-v2` 在关闭语义断句且句子结束时返回 `emo_tag` 与 `emo_confidence`。文件转写的说话人分离需开启 `diarization_enabled`，之后片段才出现 `speaker_id`。[Paraformer 服务端事件](https://help.aliyun.com/zh/model-studio/paraformer-server-events) [Paraformer 文件转写 SDK](https://help.aliyun.com/zh/model-studio/paraformer-recorded-speech-recognition-python-sdk)

Qwen3-ASR 实时事件返回 `language`、`emotion`、已确认 `text` 和可能修正的 `stash`，最终事件给 `transcript`。Qwen 实时模型目前不返回时间戳；Qwen 文件转写模型才支持词时间戳。[Qwen-ASR 服务端事件](https://help.aliyun.com/zh/model-studio/qwen-asr-realtime-server-events) [实时语音识别模型能力说明](https://help.aliyun.com/en/model-studio/real-time-speech-recognition-user-guide)

### 腾讯云 ASR

录音文件结果的 `ResultDetail[]` 是分句列表，包含 `StartMs`、`EndMs`、`Words[]`、`SpeechSpeed`、`SpeakerId`、`EmotionalEnergy`、`EmotionType`、`LangType` 等。说话人标签需请求时开启分离，或由双声道直接映射左右声道；`EmotionalEnergy` 是音量分贝值除以 10，并不等于具体情绪类别。[SentenceDetail 数据结构](https://cloud.tencent.com/document/api/1093/37824) [录音文件结果查询](https://cloud.tencent.com/document/api/1093/37822)

极速版的 `flash_result[].sentence_list[]` 同样可含片段、词时间戳和说话人字段，说明能力还取决于选用的腾讯 ASR 产品形态。[录音文件识别极速版](https://cloud.tencent.com/document/product/1093/52097)

### 百度智能云 ASR

百度短语音标准版成功响应主要是 `result` 候选字符串数组，官方字段表没有词时间戳、说话人、语言检测或置信度。实时 WebSocket 会依次返回每句话的 `MID_TEXT` 和 `FIN_TEXT`，最终句子带 `start_time`、`end_time`，但不是词级时间戳。[短语音识别标准版](https://cloud.baidu.com/doc/SPEECH/s/Jlbxdezuf) [实时语音识别 WebSocket API](https://ai.baidu.com/ai-doc/SPEECH/jlbxejt2i)

### 讯飞开放平台 ASR

流式听写通过 `data.result.ws[].cw[]` 返回字词；`ws.bg` 是 10 ms 帧单位的起点偏移，可选 `vinfo=1` 后返回 VAD 起止帧。文档明确把 `sc/wb/wc/we/wp` 列为保留字段，因此不应把示例中的 `sc` 当成稳定置信度契约。[语音听写流式版 WebAPI](https://www.xfyun.cn/doc/asr/voicedictation/API.html)

录音文件转写支持可选角色分离；结果 `lattice/lattice2` 中可见片段起止、词级位置、`wc` 分数和 `spk` 标签。自动多语种的实时大模型在 `lang=autominor` 时可在词结果返回当前语言 `lg`。[录音文件转写标准版](https://www.xfyun.cn/doc/asr/ifasr_new/API.html) [实时语音转写大模型](https://www.xfyun.cn/doc/spark/asr_llm/rtasr_llm.html)

### Google Cloud Speech-to-Text

V2 的 `results[].alternatives[]` 提供片段候选和候选置信度。词时间戳、词置信度和说话人标签分别需要开启相应配置；自动多语言选择也受模型和区域限制。[V2 Recognize 响应结构](https://docs.cloud.google.com/speech-to-text/docs/reference/rest/v2/projects.locations.recognizers/recognize) [词置信度与时间戳](https://docs.cloud.google.com/speech-to-text/v2/docs/word-confidence) [说话人分离](https://docs.cloud.google.com/speech-to-text/v2/docs/multiple-voices) [自动多语言](https://docs.cloud.google.com/speech-to-text/docs/multiple-languages)

### Azure AI Speech

实时 SDK 以 utterance 返回识别结果，Fast/Batch REST 使用 `phrases[]`，可包含 `offset`、`duration`、`locale`、`confidence`、`speaker` 和 `words[]`；具体字段取决于请求选项。说话人分离需单独配置。[Fast Transcription REST](https://learn.microsoft.com/en-us/rest/api/speechtotext/transcriptions/transcribe?tabs=HTTP&view=rest-speechtotext-2024-05-15-preview) [实时结果与时间戳](https://learn.microsoft.com/en-us/azure/ai-services/speech-service/get-speech-recognition-results) [实时说话人分离](https://learn.microsoft.com/en-us/azure/ai-services/speech-service/get-started-stt-diarization)

Azure 的语速/韵律和情感能力不应算作基础 STT 字段：前者属于发音评测，后者的官方呼叫中心方案是在 Speech 转写后再调用 Language 服务。[发音评测](https://learn.microsoft.com/en-us/azure/ai-services/Speech-Service/how-to-pronunciation-assessment) [呼叫中心方案](https://learn.microsoft.com/en-us/azure/ai-services/speech-service/call-center-quickstart)

### Amazon Transcribe

标准 Batch 输出包含整段 `transcripts[]` 和词/标点 `items[]`；发音 item 有起止时间和候选置信度。说话人分离需要开启 speaker partitioning，结果通过 `speaker_labels.segments[]` 和 item 的 `speaker_label` 表示。自动语言识别会返回识别语言及分数。[标准输出结构](https://docs.aws.amazon.com/transcribe/latest/dg/how-it-works.html) [说话人分离](https://docs.aws.amazon.com/transcribe/latest/dg/diarization.html) [语言识别请求](https://docs.aws.amazon.com/transcribe/latest/APIReference/API_StartTranscriptionJob.html)

响度、语速、打断、静音/说话时长和轮次情感属于独立的 Call Analytics 输出，不是基础 Transcribe 的标准响应；该专项输出也没有性别字段。[Call Analytics 能力](https://docs.aws.amazon.com/transcribe/latest/dg/call-analytics-batch.html) [Call Analytics JSON](https://docs.aws.amazon.com/transcribe/latest/dg/tca-output-batch.html)

### OpenAI Audio Transcriptions

默认 GPT 转写响应主要是文本；`whisper-1 + verbose_json` 才有片段，并可请求词时间戳。`gpt-4o-transcribe-diarize + diarized_json` 返回带 `start`、`end`、`text`、`speaker` 的片段，但该模型不支持词时间戳。GPT 模型可请求 token `logprobs`，Whisper 片段可有 `avg_logprob`/`no_speech_prob`，这些都不是跨模型统一的 0-1 ASR 置信度。[Audio Transcriptions API](https://platform.openai.com/docs/api-reference/audio/speech-audio-done-event?lang=curl) [GPT-4o Transcribe Diarize](https://developers.openai.com/api/docs/models/gpt-4o-transcribe-diarize)

## 对 Saymore 的直接含义

1. 不要设计一个假定所有 Provider 都能填满的扁平结果对象；应区分 `transcript`、`segments`、`words` 和可选 `analysis`，并保留“字段缺失”语义。
2. 火山流式接口优先消费已有的 `utterances/words` 和二遍结果；不要为追求字段数量切到不匹配实时输入场景的客服录音接口。
3. 做准确率对比时保存三层数据：原始流式结果、二遍最终结果、LLM 后处理结果。时间戳/说话人等元数据用于对齐和诊断，不应被当作准确率优化本身。
4. 每个 Provider 建一张“模型/接口/参数/返回字段”能力表；同一厂商不同接口的差异，往往大于两家厂商的基础文本字段差异。
