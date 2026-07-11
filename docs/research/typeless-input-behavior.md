# Typeless 语音输入交互调研

调研日期：2026-07-11

## 结论

Typeless 当前桌面版快速入门描述的主流程是：先把光标放入文本框，按一次快捷键开始口述，再按一次结束，随后 Typeless 将格式化后的口述结果插入该文本框。这个官方流程支持 Saymore 采用“录音期间不向目标输入框写入中间结果，处理完成后一次性插入最终精炼文本”的产品设计。[Your first dictation](https://www.typeless.com/help/quickstart/first-dictation)

需要严格限定这个结论：官方页面没有直接写“录音期间目标输入框始终为空”，也没有公开内部 ASR、LLM、流式处理、剪贴板或光标恢复的实现。能确认的是官方操作顺序把“插入格式化结果”放在结束口述之后；不能把这一交互事实扩展成未公开的技术架构。

## 已验证的交互事实

### 桌面端的输入时机

2026-06-11 更新的桌面快速入门给出了明确顺序：

1. 点击任意文本框，让闪烁光标留在目标位置。
2. 按一次快捷键开始，默认键是 macOS 的 `Fn`、Windows 的右 `Alt`。
3. 自然口述。
4. 再按一次快捷键结束并“see your formatted dictation”；随后 Typeless 将口述结果插入文本框。

因此，公开文档描述的是“结束后插入格式化结果”，没有描述边说边把部分转录写进目标输入框。[Your first dictation](https://www.typeless.com/help/quickstart/first-dictation)

官网把结果称为 ready-to-send / polished text，并明确列出填充词消除、重复消除、口头改口处理、措辞优化、列表与邮件格式化等 AI 自动编辑能力。安装页的示例把 “How about we meet tomorrow at, um, 7 am? Oh, actually, let's do 3 pm.” 变成 “How about we meet tomorrow at 3 PM?”。[Installation and setup](https://www.typeless.com/help/installation-and-setup) [Explore the key features](https://www.typeless.com/help/quickstart/key-features)

官网和帮助中心多次使用 “in real time” 或 “instantly”，但这些措辞没有说明目标输入框是否展示部分转录。因此它们不能作为“实时逐字写入”的证据。[Typeless homepage](https://www.typeless.com/) [Explore the key features](https://www.typeless.com/help/quickstart/key-features)

### 快捷键开始与结束

最新的桌面快速入门说明默认采用切换式操作：按一次开始，再按一次结束。[Your first dictation](https://www.typeless.com/help/quickstart/first-dictation)

但 2026-01-23 更新的 macOS 安装页在体验示例中要求用户 “Hold down the fn key or your custom keyboard shortcut”。这与较新的快速入门存在冲突。公开文档没有解释这是可选的按住说模式、旧版本行为，还是仅用于引导页演示。[Installation and setup](https://www.typeless.com/help/installation-and-setup)

因此目前可以确认 Typeless 至少公开描述过两种操作，但不能确认现行版本默认是否同时支持两种模式。若 Saymore 选择“按住说、松开结束”，应视为自己的产品决策，不能说这是已核实的 Typeless 当前默认行为。

### 文本投递与系统权限

Typeless FAQ 明确说明，桌面端使用 Accessibility 权限把文本直接插入光标所在位置，并用全局快捷键触发口述。安装页同样要求 macOS 的 Accessibility 与 Microphone 权限，并声称跨应用输入不需要复制粘贴。[FAQs](https://www.typeless.com/help/faqs) [Installation and setup](https://www.typeless.com/help/installation-and-setup)

这是关于系统能力和用户可见行为的说明，不足以判断 Typeless 内部使用 macOS Accessibility API 的哪一种调用、是否在某些应用里模拟按键，或是否把剪贴板当作兼容性后备方案。官方资料没有公开这些细节。

### 云端处理与 AI 精炼

Typeless 的 Data Controls 与 Privacy Policy 明确说明：

- 转录在云端处理，以提供准确度和低延迟。
- 处理内容包括语音音频，以及有限上下文，例如当前应用和其中相关文本。
- 音频和上下文在结果返回设备后立即丢弃，服务端声明零保留。
- Typeless 使用第三方 LLM 提供商（页面举例 OpenAI），并声明这些提供商采用零保留且不把数据用于训练。

这些资料证明 Typeless 的成品文本不只是设备上的纯 ASR 输出，也证明服务可能结合 LLM 与有限应用上下文。但它们没有公开 ASR 和 LLM 的供应商、模型、提示词、调用次数、处理顺序，或究竟在录音中还是录音结束后开始精炼。[Data Controls](https://www.typeless.com/data-controls) [Privacy Policy](https://www.typeless.com/privacy)

### 恢复与错误行为

如果用户错过复制结果或误关结果卡片，可以从应用的 History 找回近期转录。官方说明 History 存在本机，并按用户选择的保留周期自动清理。[Where did my transcript go?](https://www.typeless.com/help/troubleshooting/missing-transcript)

每次口述当前最长 9 分钟；第 8 分钟时语音栏显示 60 秒倒计时；达到上限后，已有内容自动保存到 History。文档没有承诺此时自动插入目标文本框。[How long can I dictate for?](https://www.typeless.com/help/troubleshooting/dictation-limit)

当麦克风不可用时，应用会显示明确错误，并引导用户检查系统权限、设备选择、其他占用麦克风的应用和硬件连接。该帮助页分别给出 macOS 与 Windows 的排查步骤。[Microphone unavailable](https://www.typeless.com/help/troubleshooting/microphone-unavailable)

没有找到一手资料说明以下失败策略：

- 网络、ASR 或 LLM 超时时是否插入原始转录；
- AI 精炼失败时是否自动降级；
- 结束口述后用户切换窗口或移动光标时如何处理；
- Accessibility 注入失败时是否改用剪贴板；
- 目标输入框失焦时结果卡片、History 与自动重试之间的优先级。

同样没有找到一手资料证明 Typeless 提供独立的“撤销上次投递”命令或全局快捷键。FAQ 只确认 Accessibility 用于向光标位置插入文字，History 文档只说明如何找回近期转录；用户是否依赖目标应用原生撤销、Typeless 是否将插入记录为单个撤销步骤，公开资料均未说明。[FAQs](https://www.typeless.com/help/faqs) [Where did my transcript go?](https://www.typeless.com/help/troubleshooting/missing-transcript)

因此，Saymore 的降级与安全投递规则需要独立设计，不能仿照一个未公开的 Typeless 行为。

## 平台差异

桌面快速入门同时覆盖 macOS 和 Windows：默认快捷键分别是 `Fn` 与右 `Alt`，操作流程相同。macOS 文档明确依赖 Accessibility；Windows 公开资料没有给出与 macOS Accessibility 等价的具体文本注入机制。[Your first dictation](https://www.typeless.com/help/quickstart/first-dictation) [FAQs](https://www.typeless.com/help/faqs)

Typeless 也提供 iOS 和 Android，并将移动端产品描述为 AI voice keyboard；桌面端则是跨应用的 voice dictation 应用。此次查到的官方移动端发布说明没有足够细节确认它们是否也只在最终精炼后插入、是否展示部分转录，或怎样处理应用切换。因此不能把桌面端的插入时机直接外推到移动端。[Downloads](https://www.typeless.com/downloads) [iOS release notes](https://www.typeless.com/help/release-notes/ios) [Android release notes](https://www.typeless.com/help/release-notes/android)

## 对 Saymore 的直接启示

可以采用以下与已验证桌面体验一致的产品合同：

```text
目标文本框获得焦点
  -> 开始录音
  -> 录音期间只显示录音/处理状态，不写入目标文本框
  -> 用户结束录音
  -> ASR
  -> 可选规则标准化
  -> 可用时进行可选 LLM 精炼
  -> 一次性向目标文本框插入最终结果
```

这会消除实时部分转录反复改写、光标漂移和精炼后整段替换的问题。它与 Typeless 最新桌面快速入门描述的“结束后插入格式化口述”一致，但具体技术实现仍应由 Saymore 自己验证。

建议把“目标框中的实时逐字稿”从当前产品设计删除。可选浮层如果保留，应只表达录音中、处理中、失败和可恢复状态；是否展示部分转录是另一个产品选择，不是完成基础输入闭环的必要条件。

## 仍然未知

- Typeless 是否在语音栏内部展示未确认的部分转录。
- 它是否采用流式 ASR，以及何时开始 AI 精炼。
- 它是否只注入一次，还是内部先写入隐藏/临时内容再替换。
- 它如何保存原始焦点、定位插入点并检测用户在处理期间的编辑。
- 它是否读取选区、当前文本框、窗口内容或更广的屏幕内容；隐私文档只说“当前应用和其中相关文本”。
- 它的剪贴板使用、Accessibility 调用和跨应用兼容性后备路径。
- 现行桌面版是否提供按住说与切换式两种快捷键模式。

回答这些问题需要运行产品并进行黑盒测试，或获得 Typeless 官方更具体的工程说明；现有公开一手资料不足以作结论。

## 一手来源

- [Your first dictation](https://www.typeless.com/help/quickstart/first-dictation), Typeless Help Center，页面内容更新时间 2026-06-11。
- [Installation and setup](https://www.typeless.com/help/installation-and-setup), Typeless Help Center，页面内容更新时间 2026-01-23。
- [Explore the key features](https://www.typeless.com/help/quickstart/key-features), Typeless Help Center，页面内容更新时间 2026-01-20。
- [FAQs](https://www.typeless.com/help/faqs), Typeless Help Center，页面内容更新时间 2026-06-23。
- [Where did my transcript go?](https://www.typeless.com/help/troubleshooting/missing-transcript), Typeless Help Center，页面内容更新时间 2026-01-21。
- [How long can I dictate for?](https://www.typeless.com/help/troubleshooting/dictation-limit), Typeless Help Center，页面内容更新时间 2026-06-23。
- [Microphone unavailable](https://www.typeless.com/help/troubleshooting/microphone-unavailable), Typeless Help Center，页面内容更新时间 2026-06-01。
- [Data Controls](https://www.typeless.com/data-controls), Typeless 官方数据说明。
- [Privacy Policy](https://www.typeless.com/privacy), Typeless 官方隐私政策，页面标示生效日期 2026-03-13。
- [Downloads](https://www.typeless.com/downloads), Typeless 官方下载页。
- [Typeless homepage](https://www.typeless.com/), Typeless 官方产品页。
