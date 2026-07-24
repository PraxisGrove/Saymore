# 声忆 VoiceInput 本地排版与 AI 整理实现分析

- 调研日期：2026-07-24
- 样本版本：VoiceInput 0.79.0（build 219）
- 范围：官网、帮助文档、更新日志、官方分发安装包，以及官网点名引用的排版规范与开源项目。本文只讨论文本后处理，不评价
  ASR 准确率。

## 结论

声忆的“简单润色”并非单一正则，也不是秘密的小模型包办全部工作，而是至少三层明确分工：

```text
ASR 原文
  -> 本地确定性处理：正则/Unicode 字符类 + 规则顺序 + 品牌词典
  -> 本地标点模型：sherpa-onnx CT-Transformer（模型另行下载）
  -> 可选 LLM：同音字、口头禅、上下文纠错、流水句拆分
  -> 本地规则再次收口并输出
```

官网明确把“空格、标点、大小写”归给本地引擎，把“同音字、口头禅”等语义判断归给
LLM，并宣称 24 条排版规则全部跑完低于 5
ms、零网络调用、每条可单独开关。[声忆官网：中文排版](https://voiceinput.app/zh/#typography)

对官方 0.79.0 安装包的只读静态检查进一步确认：

- 应用是原生 Swift/SwiftUI，而非网页壳；存在
  `ChineseTypographyEngine`、`EnglishTypographyEngine`、`BrandCaseLibrary` 和
  `NSRegularExpression`。
- 中文规则 ID 和公开示例能对应上确定性替换；二进制中可见 `\p{Han}`、ASCII
  字符类、前后向断言等正则模式。
- 独立存在 `PunctuationService` 和 `PunctuationModelManager`，使用 sherpa-onnx
  的中英 CT-Transformer 标点模型，模型约 281 MB、按需下载。
- 当前版本已经有英文排版引擎。因此“它目前只参考中文”只适用于官网重点展示或旧版本印象，不适用于
  0.79.0 客户端能力。

最准确的回答是：**截图中的 24 条“排版规则”主体是本地确定性规则和词典，不需要
LLM；上下文标点可能使用本地神经网络模型；真正涉及语义取舍的润色才走 LLM。**

## 证据等级

| 等级             | 含义                                                                   |
| ---------------- | ---------------------------------------------------------------------- |
| 官方公开事实     | 官网、帮助文档或更新日志明确说明的产品行为                             |
| 官方制品静态证据 | 对官方签名、公证、公开下载的 0.79.0 安装包做只读字符串、符号和依赖检查 |
| 推断             | 根据公开行为、规则名和依赖解释实现方式；不是开发者公布的源代码         |

本文不会把“参考某份指南”写成“逐条实现该指南”，也不会把二进制中的类名写成完整调用顺序。

## 官方明确披露了什么

### 本地规则与 LLM 是两条不同路径

官网的直接表述是：LLM
管语义（同音字、口头禅），本地引擎管格式（空格、标点、大小写）；本地规则不走网络、低于
5 ms，并可逐条开关。[声忆官网](https://voiceinput.app/zh/)

官方对比页把本地排版进一步收窄为“中英空格、品牌大小写、单位空格”，并说明可选 AI
整理在停顿后触发，不阻塞首次落字。[声忆 vs 讯飞输入法](https://voiceinput.app/zh/vs/iflytek/)

AI
整理帮助页则把以下工作明确交给大模型：去口语助词、补缺失标点、根据上下文修同音字、拆分流水句；同时禁止补写、改意、翻译或回答用户的问题。[AI 整理帮助](https://voiceinput.app/zh/help/ai-tidy)

### 官网只展示 7 个典型例子

“24 rules”不是公开的完整规则表。主页实际只展示：

| 输入                 | 输出                    | 可确定的技术类别                   |
| -------------------- | ----------------------- | ---------------------------------- |
| `今天3点开会`        | `今天 3 点开会`         | 汉字与数字边界                     |
| `用kimi做个api设计`  | `用 Kimi 做个 API 设计` | 边界空格 + 品牌/缩写词典           |
| `你好,世界`          | `你好，世界`            | 中文语境半角标点转全角             |
| `让我想想...`        | `让我想想……`            | 省略号规范化                       |
| `好的！！！`         | `好的！`                | 连续标点折叠                       |
| `他说"好"`           | `他说「好」`            | 引号风格替换                       |
| `deepseek响应约50ms` | `DeepSeek 响应约 50 ms` | 品牌词典 + 双侧空格 + 数字单位空格 |

这些变换输入输出确定、局部可判定，非常适合正则、Unicode 分类和词典完成；没有调用
LLM 的必要。

### 更新日志暴露了演进过程

官方[完整更新日志](https://voiceinput.app/zh/changelog/)提供了重要边界：

- v0.14.0：空格、标点、大小写改为本地处理；加入 45+ 品牌名大小写修正。
- v0.21.2：中英文空格改成更窄的“半格”，新增破折号、引号、省略号等 5 条细节规则。
- v0.23.3：再次明确中英文半格、品牌大小写，并要求保留原文已有标点。
- v0.35.1：本地流式识别开始实时加标点。
- v0.75.2：明确提到“本地标点模型”及自定义替换规则。
- v0.75.4：英文内容自动套英文排版。
- v0.75.8：逐条规则和统计移入高级设置，并显示本月修正次数。

因此，官网“本地引擎管标点”至少包含两种实现：字符级标点规范化，以及独立的上下文标点模型。不能把两者都概括成正则。

## 0.79.0 安装包静态证据

### 样本与方法

- 官方下载：`https://dl.voiceinput.app/VoiceInput_v0.79.0.dmg`
- DMG
  SHA-256：`dcb238ed47b598cb43e489e7147c535c0489621b46574fdfaec55849fa75cdc9`
- 主程序
  SHA-256：`6c63b88a22fd8e9ca9d5d038f8874d781440ead5d360deee72227ca757697308`
- Bundle ID：`com.jiangfyi.voiceinput`
- 架构：arm64 + x86_64，Apple 公证票据已 stapled
- 方法：`plutil`、`otool -L`、`codesign`、`strings`、`nm`；没有运行应用、修改文件或绕过保护。

主程序链接 SwiftUI、SwiftData、AppKit、Foundation 等系统框架，并含 Swift
类型名；没有 Electron 或 WebView 应用壳迹象。这支持“本地规则直接编译进原生 Swift
客户端”。

### 中文确定性规则

二进制可恢复出以下规则 ID。它们是 0.79.0 的制品事实，但名称不等于公开 API：

| 规则 ID                                                                   | 含义                       |
| ------------------------------------------------------------------------- | -------------------------- |
| `r1_pangu_space_cjk_to_ascii`                                             | 汉字到 ASCII 边界空格      |
| `r2_pangu_space_ascii_to_cjk`                                             | ASCII 到汉字边界空格       |
| `r5_collapse_whitespace`、`r6_trim`                                       | 合并多余空白、首尾裁剪     |
| `r7_cjk_punct_to_full`                                                    | 中文语境标点全角化         |
| `r8_eng_punct_space`                                                      | 英文标点后的空格           |
| `r9_trim_space_around_full_punct`                                         | 去掉全角标点旁空格         |
| `r10_merge_duplicate_punct`                                               | 合并重复标点               |
| `r11_ellipsis`                                                            | 省略号规范化               |
| `r12_em_dash`、`r12b_em_dash_dedupe`                                      | 破折号规范化及去重         |
| `r13_chinese_quotes`、`r23_corner_to_curly_quote`                         | 中文引号风格转换           |
| `r14_smart_parentheses`                                                   | 括号处理                   |
| `r15_full2half_digits`、`r16_full2half_letters`、`r17_full_space_to_half` | 全角数字、字母、空格转半角 |
| `r19_number_unit_space`                                                   | 数字与单位间距             |
| `r20_time_colon_half`                                                     | 时间冒号半角化             |
| `r21_strip_indent`                                                        | 清理缩进                   |
| `r22_chinese_range_dash`                                                  | 中文范围连接号             |
| `r24_auto_end_period`                                                     | 自动句末句号               |

制品中同时存在 `NSRegularExpression` 类型、`\p{Han}`、ASCII
字符类、前后向断言，以及类似 `([,.!?;:])([a-zA-Z0-9])`
的模式。这足以确认正则是主要机制之一。规则拥有独立
ID、启用集合和命中统计，说明实现更可能是**有顺序的规则管线**，而非一个大正则一次替换。

品牌大小写由独立 `BrandCaseLibrary`
和用户品牌缓存承担。它本质上是规范写法词典与边界匹配：适合
`kimi -> Kimi`、`deepseek -> DeepSeek`、`api -> API`；ASR
把品牌听成完全不同汉字时，仍需要热词、纠错词典或 LLM。

### 英文排版引擎已经存在

0.79.0 二进制含 `EnglishTypographyEngine.enabledRules`，可恢复出至少九类行为：

1. 合并连续空格。
2. 裁剪首尾空格。
3. `--` 转 em dash。
4. `...` 转 ellipsis。
5. 删除句读符前空格。
6. 在句读符后补一个空格。
7. 恢复常见缩写中的 apostrophe，例如 `dont -> don't`、`im -> I'm`。
8. 大写独立的 `i` 和句首字母。
9. 将直引号转为弯引号。

其中空白、标点和直/弯引号转换是低风险确定性格式化；缩写 apostrophe
恢复和句首大写已经带少量词法/句法判断，仍可本地实现，但必须有例外和保护区，不能使用无边界的全局替换。

### 本地标点模型不是正则

制品含 sherpa-onnx、ONNX
Runtime、`PunctuationService`、`PunctuationModelManager` 和
`SherpaOnnxOfflinePunctuationWrapper`。模型下载地址指向 sherpa-onnx
官方发布的中英 CT-Transformer 标点模型：

`https://huggingface.co/csukuangfj/sherpa-onnx-punct-ct-transformer-zh-en-vocab272727-2024-04-12/resolve/main/model.onnx`

日志字符串标注下载量约 281 MB。官方 changelog
也明确说新安装用户会下载“本地标点模型”。这层适合根据上下文决定逗号、句号和问号位置；字符替换规则则负责把模型或
ASR 已有标点规范成目标风格。

## 引用指南究竟提供了什么

官网称其参考 pangu.js、chinese-copywriting-guidelines、W3C CLReq 和 GB/T
15834-2011，但没有声称直接链接、移植或完整实现任一项目。

- [pangu.js](https://github.com/vinta/pangu.js)的公开实现本身就是一系列 Unicode
  字符类、正则替换、保护占位符和有序处理，用于在 CJK
  与半角字母、数字、符号之间加空白。其[核心源码](https://github.com/vinta/pangu.js/blob/master/src/shared/index.ts)证明这类功能无需模型。
- [中文文案排版指北](https://github.com/sparanoid/chinese-copywriting-guidelines)给出中英文/数字间距、数字与单位空格、全角标点旁不留空格、正确品牌大小写等可操作规则；它是风格指南，不是解析器。
- [W3C CLReq](https://www.w3.org/TR/clreq/)描述中文排版的字符、标点、混排、行首行尾禁则和区域差异。它讨论的是排版要求，不规定应用必须采用正则、状态机或模型；其中很多行布局规则也不适合由纯文本清理器实现。
- 国家标准平台确认
  [GB/T 15834-2011《标点符号用法》](https://openstd.samr.gov.cn/bzgk/std/newGbInfo?hcno=22EA6D162E4110E752259661E1A0D0A8)为现行标准；它规定书面标点用法，不是实现算法。

特别需要注意：W3C CLReq 讨论的中西间距是字体排版中的比例宽度，例如默认可为 1/4
em；官网却把输出字符标为 U+2009。官网静态示例 HTML 实际使用的是 U+00A0
NBSP。三者不是同一概念，不能仅凭截图断言客户端一定输出 U+2009。

## 源码是否公开

截至调研日，官网、帮助页、sitemap
和安装包元数据均没有指向声忆客户端源码仓库；针对产品名、域名和 Bundle ID 的公开
GitHub
检索也没有找到可归属的应用源码。可以识别出的[官方 Homebrew tap](https://github.com/voiceinput-app/homebrew-tap)只包含版本、DMG
URL、SHA、应用名和系统要求等打包元数据，不含排版引擎或规则源码。

可以确认公开的是：官网页面、更新日志、下载制品，以及它引用的第三方规范/项目。不能确认公开的是：24
条规则的正式源代码、执行顺序、所有例外、回归测试和提示词版本。因此，本报告的规则名来自官方制品静态证据，不应被描述成“官方开源实现”。

## 对技术判断的置信度

| 判断                              | 置信度 | 依据                                                           |
| --------------------------------- | ------ | -------------------------------------------------------------- |
| 24 条简单排版主体为本地确定性规则 | 高     | 官网分工 + 规则 ID + 正则模式 + 独立启用集合                   |
| 品牌大小写使用词典和边界匹配      | 高     | `BrandCaseLibrary`、用户品牌缓存、45+ 品牌更新记录             |
| 上下文标点可由本地模型完成        | 高     | 官方 changelog + sherpa-onnx wrapper + CT-Transformer 下载地址 |
| 中文排版直接使用 pangu.js 包      | 低     | 原生 Swift 制品中无 pangu 依赖标识；官网只说“参照”             |
| 全部 24 条都能从官网七个例子还原  | 低     | 官网未公开完整清单；只能从 0.79.0 制品恢复大部分规则名         |
| 客户端实际输出 U+2009             | 未确认 | 营销文案、HTML 示例和字体排版概念不一致                        |

## 可复用的架构启示

声忆最值得借鉴的不是某一条正则，而是责任边界：

1. **确定且可逆的书写规范本地做。** Unicode
   规范化、空白、全半角、标点邻接、重复标点、数字单位、品牌标准写法，应使用有
   ID、有顺序、可单测、可关闭、可统计的规则。
2. **需要上下文但不需要生成的标点，可选本地模型。** 这能覆盖无网络听写，但增加约
   281 MB 模型、下载和冷启动成本，不应混入轻量规则引擎的性能口径。
3. **涉及语义删除和重构才交给 LLM。**
   口头禅是否为空、自我纠正保留哪段、同音字消歧、流水句拆分、邮件或列表结构都需要上下文。
4. **LLM 之后仍跑确定性收口。** 品牌写法、Unicode
   标点、空格和输出包装可以在模型输出后再次校验，获得稳定风格。
5. **每条规则都需要保护区和反例。**
   URL、邮箱、文件路径、代码、版本号、小数、Markdown、emoji、单位例外和用户原有标点都不能被普通边界正则误伤。

## 一手来源

- [声忆官网](https://voiceinput.app/zh/)
- [声忆完整更新日志](https://voiceinput.app/zh/changelog/)
- [声忆 AI 整理帮助](https://voiceinput.app/zh/help/ai-tidy)
- [声忆 vs 讯飞输入法](https://voiceinput.app/zh/vs/iflytek/)
- [声忆官方 Homebrew tap](https://github.com/voiceinput-app/homebrew-tap)
- [pangu.js 官方仓库](https://github.com/vinta/pangu.js)及[核心源码](https://github.com/vinta/pangu.js/blob/master/src/shared/index.ts)
- [中文文案排版指北](https://github.com/sparanoid/chinese-copywriting-guidelines)
- [W3C Requirements for Chinese Text Layout](https://www.w3.org/TR/clreq/)
- [全国标准信息公共服务平台：GB/T 15834-2011](https://openstd.samr.gov.cn/bzgk/std/newGbInfo?hcno=22EA6D162E4110E752259661E1A0D0A8)
- [sherpa-onnx 官方仓库](https://github.com/k2-fsa/sherpa-onnx)
