# Typeless 与闪电说多语言支持调研

调研日期：2026-07-11

## 结论先行

两款产品并不是用同一种产品架构实现“多语言”。

- **Typeless 是统一托管体验**：普通听写宣称支持 100+ 种语言，用户直接说，产品自动检测语种并按该语种转写；区域变体可以手动指定。模型、ASR Provider 和处理编排不向用户开放。
- **闪电说是可替换模型体验**：产品把“语音识别模型”和“快速大模型”明确拆开，允许使用默认会员模型、自接云端 ASR 或本地 SenseVoice。其实际语种覆盖因此取决于当前 ASR Provider/模型，而不是一个跨 Provider 的统一语言清单。
- 两者都不应被描述为“LLM 直接完成语音识别”。公开资料支持的流程是 **ASR 先把声音变成文字，之后 AI/大模型负责纠错、格式化、术语和表达整理**。Typeless 没有公开两阶段的具体内部边界；闪电说则在模型设置中明确展示了这三类模型的职责。
- **Typeless 明确承诺普通听写自动识别语言**；闪电说的一手资料没有找到产品层的自动检测开关、固定支持数量、完整语言列表或中英混说承诺。不能根据底层模型宣传替产品作出这些承诺。

## 对照表

| 维度 | Typeless | 闪电说 |
| --- | --- | --- |
| 支持范围 | 官方称普通听写支持 100+ 语言，未在查到的页面列出完整清单 | 未找到统一数量或完整清单；能力随所选 ASR 模型变化 |
| 普通听写语言选择 | 自动检测，用户“直接说” | 官方未说明统一行为；不同 ASR Provider 可能不同 |
| 区域变体 | macOS 可为某种语言选择地区变体，例如 English (UK/US)、Spanish (Mexico) | 未找到同类产品级设置；v0.7.5 明确新增繁体中文支持 |
| 中英混说 | 未找到官方明确承诺；“自动检测语言”不能证明同一句内 code-switching | 未找到产品级明确承诺；词典可纠正英文缩写，但这不等于混说能力保证 |
| 默认输出语言 | 普通听写保持检测到的语言；翻译模式另行指定目标语言 | 直接说的输出受 ASR、快速大模型、风格和词典影响；未找到统一输出语言规则 |
| 翻译 | macOS 有独立 Translation mode，手动选目标语言，使用独立快捷键 | 文档中的“跨语言沟通”属于长按帮我说/技能，根据屏幕消息生成对应语言回复，不是纯 ASR 听写 |
| ASR 与 LLM | 官方披露云端转录并使用第三方 LLM 做产品能力，但未公开模型、供应商组合和调用顺序 | 明确拆为语音识别模型、快速大模型、高级大模型；短按最终文本由前两者共同影响 |
| 本地/云端 | 官方说明转录在云端处理 | 可用默认会员云模型、自接云 ASR，或本地 SenseVoice；本地 ASR 可完全离线 |
| 用户自定义模型 | 未公开提供自定义 ASR/LLM Provider | 支持自定义模型；官方教程列出火山豆包流式 ASR，并提到阿里 qwen3-asr；也支持本地 SenseVoice |
| 词典 | Personal Dictionary 添加名称和术语；官方未公开它注入 ASR 还是后处理 | 词典用于人名、品牌名、英文缩写和术语；官方说明快速大模型开启时词典才生效，且发布说明提到云端识别引擎的词典稳定性优化 |
| 平台 | 查到的自动检测说明是通用功能页；语言变体与 Translation mode 页面明确针对 macOS；移动端另有翻译功能 | 文档给出 macOS、Windows 本地模型目录；官网当前还列出 Android/Linux 下载入口，但未找到各平台多语言行为对照 |

## Typeless：已验证行为

### 普通听写自动检测 100+ 语言

Typeless 的功能页明确写出 “Dictating in multiple languages (100+ supported)”，并说明用户只需使用任意语言说话，Typeless 会自动检测并用该语言转写。该页面没有提供完整语言列表，也没有要求用户在每次听写前手动选源语言。[Explore the key features](https://www.typeless.com/help/quickstart/key-features)

这里能确认的是**每次普通听写的语言自动检测**。不能进一步确认：

- 自动检测是在 ASR 前、ASR 内，还是由后处理纠正；
- 一段话里切换两种语言时是否保留两种文字；
- 对所有 100+ 语言是否使用同一 ASR 模型；
- 各语言是否有相同准确率和功能覆盖。

### 自动检测与区域变体可以并存

macOS v0.8.1 增加了 preferred language variant 设置。用户先选择语言，再选择地区变体，例如 Spanish (Mexico)；官方解释这会帮助产品理解口音和地区词汇，并决定 `colour`/`color` 一类地区拼写。因此，Typeless 的体验并非“完全没有语言设置”，而是**普通语种自动检测，地区偏好可显式覆盖**。[Setting your preferred language variant](https://www.typeless.com/help/release-notes/macos/more-language-variants-supported)

官方页面没有说明变体偏好是 ASR 提示、词汇表、LLM 提示还是最终拼写规则。

### 翻译模式和多语言听写是两个功能

macOS Translation mode 要求用户在设置中选择**目标语言**，再使用单独快捷键（页面所示默认 `fn + Shift`）说话；Typeless 随后插入格式化后的目标语言译文。普通听写则保持检测到的源语言。两者不能混为“自动多语言识别”。[Getting started with Translation mode](https://www.typeless.com/help/release-notes/macos/translation-mode)

iOS 和 Android 也有独立的 Translate 发布说明，证明移动端存在翻译能力；但这些发布页不足以证明移动端与 macOS 使用相同快捷方式、语言变体设置或内部处理链。[iOS Translate](https://www.typeless.com/help/release-notes/ios/translate) [Android Translate](https://www.typeless.com/help/release-notes/android/translate)

### 云端处理，内部模型未公开

Typeless 的 Data Controls 声明转录在云端处理，并会处理语音及有限的应用上下文；结果返回后，服务端对内容执行零保留。Privacy Policy 与 Data Controls 还说明产品使用第三方 LLM Provider（示例为 OpenAI），但没有公布 ASR Provider、语言识别模型、LLM 型号、提示词或调用编排。[Data Controls](https://www.typeless.com/data-controls) [Privacy Policy](https://www.typeless.com/privacy)

因此，合理推断是产品服务端拥有一个多语言 ASR/转录阶段和一个 AI 精炼阶段，但**无法从公开资料确认它们是严格串行、是否共享模型，或语言检测由哪一层承担**。

### 个人词典不等于热词实现已公开

功能页允许用户在 Personal Dictionary 中添加名字、术语等自定义词。它证明产品会利用个人词汇改善最终输出，但官方没有说明这些词是作为 ASR hotword、解码 bias、后处理替换还是 LLM 上下文使用。[Explore the key features](https://www.typeless.com/help/quickstart/key-features)

## 闪电说：已验证行为

### 多语言能力来自可替换 ASR，而非固定产品清单

闪电说的模型文档把处理职责分为：

1. 语音识别模型：声音转文字；
2. 快速大模型：修正错字、标点、专有名词并整理短按输出；
3. 高级大模型：理解屏幕和指令，处理长按“帮我说”。

用户可以使用默认会员模型或自定义模型。本地语音识别示例为 `sensevoice-small`；官方云端教程支持配置火山引擎“豆包流式语音识别模型 2.0”，并提到阿里 `qwen3-asr`。这意味着语种覆盖、自动检测、code-switching 和延迟都可能随 Provider 改变。[模型](https://shandianshuo.cn/docs/beginner/model) [接入语音识别模型教程](https://shandianshuo.cn/docs/faq/cloud-speech-model) [手动安装本地语音识别模型](https://shandianshuo.cn/docs/faq/local-speech-model-install)

官方本地安装页把 SenseVoice 称为“多语言语音理解模型”，但没有在闪电说文档中列出产品承诺的语种集合，也没有说明闪电说是否启用了底层模型的全部语言。因此，本报告不把模型仓库的能力外推为闪电说产品承诺。

### 繁体中文是明确增加过的产品能力

v0.7.5 发布说明明确写了“新增繁体中文支持”，同时新增阿里 Fun-ASR realtime 和 Fun-ASR Flash。这证明闪电说会在产品版本层逐步补齐语言/文字变体，而非仅依赖一个永远固定的底层模型。[闪电说 Changelog](https://shandianshuo.featurebase.app/changelog)

但发布说明没有解释繁体支持发生在 ASR、快速大模型、输出转换还是三者组合，因此实现层仍未知。

### 跨语言沟通不是纯听写多语言

闪电说入门文档的“跨语言沟通”场景是：用户长按快捷键，要求“用对方的语言回复”；应用结合屏幕上的英文、日文、繁体中文或其他语言消息，使用相应语言和语气生成回复。文档还建议开启“跨语言沟通”个人技能。[快速开始](https://shandianshuo.cn/docs)

这是高级大模型/技能驱动的**生成与翻译**，不是短按“直接说”的 ASR 语言自动检测。对只做语音输入的 Saymore，这项能力不应进入 MVP 的多语言定义。

### 本地与云端路径均存在

隐私页明确说明：配置本地语音模型后，录音和转写全程在电脑上完成且可离线；自接模型时数据从用户电脑直达所选服务商，不经过闪电说；默认会员模型则经闪电说转发，但官方称不留存内容。[隐私优先](https://shandianshuo.cn/privacy-first)

因此“闪电说支持某语言”必须带上模型路径：本地 SenseVoice、某个自接云 ASR 和会员默认 ASR 可能不是同一个覆盖集合。

### 词典横跨后处理，而非纯 ASR 热词

入门文档建议把人名、品牌名、产品名、英文缩写和行业术语加入词典。v0.7.5 的说明又明确提示：开启快速大模型时，风格和词典才会生效；较早发布说明同时提到优化阿里云识别引擎，防止用户词典错误输出。这说明词典至少参与最终文本整理，并可能与特定云 ASR 适配，但公开资料不足以定义一个所有 Provider 共用的 hotword API。[快速开始](https://shandianshuo.cn/docs) [闪电说 Changelog](https://shandianshuo.featurebase.app/changelog)

## 合理推断与未知边界

### 合理推断

- Typeless 为保持“直接说、自动检测”的统一体验，语言检测和模型路由大概率由其云端服务隐藏管理；区域变体则作为用户偏好进入识别或精炼链路。
- 闪电说更像一个语音处理编排器：统一输入/输出合同之下，不同 ASR Adapter 提供各自能力；快速大模型再处理错字、专有名词和格式。
- 闪电说词典不是简单的全局字符串替换，因为官方同时提及快速大模型生效条件和特定 ASR 引擎稳定性优化。

以上均为从产品合同推导出的架构解释，不是厂商公开的源代码事实。

### 仍然未知

- 两款产品是否可靠支持**同一句内**中文与英文自由切换。
- Typeless 的 100+ 完整语言列表、各语言质量等级和不同平台是否完全一致。
- Typeless 使用的 ASR Provider、语言识别算法、模型路由和 LLM 精炼顺序。
- 闪电说默认会员 ASR 的底层 Provider、语言列表、自动检测策略和版本锁定方式。
- 闪电说针对每个自定义 ASR 是否声明能力元数据，还是只提供配置表单。
- 两款产品如何处理无法确定语种、近音跨语言专有词、同语种不同文字系统，以及词典词条的语言归属。

## 对 Saymore 的技术启示

不要把“支持多语言”设计成一个全局布尔值，也不要把特定模型的语言清单写死为整个产品的承诺。更稳妥的合同是由 ASR Provider 报告能力：

```text
AsrCapabilities
  supported_languages
  automatic_language_detection
  intra_utterance_code_switching
  regional_variants
  hotwords
  streaming
  offline
```

产品层可以提供三种语言策略：

```text
AutoDetect
Preferred(language, variant)
Fixed(language, variant)
```

- `AutoDetect` 只有在当前 Provider 明确支持时才可用。
- `Preferred` 允许自动检测，但给中文/英文或地区变体一个偏好，用于解决短句歧义。
- `Fixed` 为单语模型或准确率优先用户锁定语种。
- LLM 精炼接收 ASR 已判定的语言和用户的输出文字偏好，但不承担“伪装 ASR 支持”的责任。
- 词典条目应允许可选语言标签；Provider 支持 hotword 时下推到 ASR，不支持时再由规则或 LLM 精炼处理。

首版验收应按**模型配置**建立测试矩阵，而不是只写“支持中文、英文”：普通话、英语、普通话夹英文术语、纯英文夹中文名称、简繁输出、短句语言歧义分别测试本地与云端 Provider。只有通过测试的组合才展示为“正式支持”；其他底层模型理论支持的语言可标为实验性。

## 一手来源

- [Typeless: Explore the key features](https://www.typeless.com/help/quickstart/key-features)
- [Typeless: Setting your preferred language variant](https://www.typeless.com/help/release-notes/macos/more-language-variants-supported)
- [Typeless: Getting started with Translation mode](https://www.typeless.com/help/release-notes/macos/translation-mode)
- [Typeless: Data Controls](https://www.typeless.com/data-controls)
- [Typeless: Privacy Policy](https://www.typeless.com/privacy)
- [Typeless: iOS Translate](https://www.typeless.com/help/release-notes/ios/translate)
- [Typeless: Android Translate](https://www.typeless.com/help/release-notes/android/translate)
- [闪电说：快速开始](https://shandianshuo.cn/docs)
- [闪电说：模型](https://shandianshuo.cn/docs/beginner/model)
- [闪电说：接入语音识别模型教程](https://shandianshuo.cn/docs/faq/cloud-speech-model)
- [闪电说：手动安装本地语音识别模型](https://shandianshuo.cn/docs/faq/local-speech-model-install)
- [闪电说：隐私优先](https://shandianshuo.cn/privacy-first)
- [闪电说 Changelog](https://shandianshuo.featurebase.app/changelog)
