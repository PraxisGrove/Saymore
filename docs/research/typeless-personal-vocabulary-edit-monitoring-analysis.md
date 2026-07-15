# Typeless 自动个人词典与输入框纠正监测分析

调研日期：2026-07-15

## 结论摘要

1. **已证实：Typeless 会把用户在语音输入之后纠正的词用于个人词典。**macOS v0.4.0 发布说明明确写道，用户在说完后纠正一个词，Typeless 会自动将它保存到个人词典；Windows v0.9.1 也明确称会捕捉用户说完后纠正的词，使姓名、术语和偏好拼写出现在后续结果中。[macOS 发布说明](https://www.typeless.com/help/release-notes/macos)；[Windows 发布说明](https://www.typeless.com/help/release-notes/windows)
2. **已证实：Typeless 能读取目标应用中的有限相关文本，并能把结果直接插入光标所在文本框。**其隐私政策和 Data Controls 将输入时处理的数据描述为应用名称及应用内“相关文本”；其帮助中心称 macOS Accessibility 权限用于直接插入文本和响应 Fn 快捷键。[Privacy Policy](https://www.typeless.com/privacy)；[Data Controls](https://www.typeless.com/data-controls)；[FAQ](https://www.typeless.com/help/faqs)
3. **尚未证实：Typeless 是否通过持续监听目标输入框的每一次编辑来实现自动加词。**官方没有说明它是否订阅 Accessibility 文本变化事件、记录按键、轮询输入框、仅跟踪刚插入的文本范围，或在下一次唤起 Typeless 时再读取上下文并比对。
4. **最合理的产品级推断：它至少建立了“本次 Typeless 输出”和“用户之后的纠正”之间的关联。**用户观察到改完几个词后词条自动出现，与官方“correct a word after speaking”的说明完全一致。实现上可能是短期跟踪刚插入区域，也可能是下一次使用时比对；现有一手资料不足以在二者之间定论。
5. **历史、上下文、写作风格学习是三条不同能力。**历史转录存储在本机且按用户选择的期限删除；上下文在使用 Typeless 时被实时处理；写作风格个性化会随使用逐渐学习并可关闭。官方没有说明自动词典是通过扫描 History、长期保存屏幕上下文或写作风格模型产生的。[历史记录说明](https://www.typeless.com/help/troubleshooting/missing-transcript)；[个性化说明](https://www.typeless.com/help/release-notes/macos/personalized-smarter)

## 证据分级

| 问题 | 结论 | 证据级别 | 官方依据 |
|---|---|---|---|
| 是否自动加入个人词典 | 是 | 已证实 | 官网称个人词汇可自动或手动加入；macOS 发布说明明确称纠正后自动保存。[官网](https://www.typeless.com/)；[macOS 发布说明](https://www.typeless.com/help/release-notes/macos) |
| 自动加入的核心触发信号 | 用户在语音输入后纠正词语 | 已证实到产品语义 | macOS：“correct a word after speaking”后自动保存；Windows 称会捕捉“words you correct after speaking”。[macOS 发布说明](https://www.typeless.com/help/release-notes/macos)；[Windows 发布说明](https://www.typeless.com/help/release-notes/windows) |
| 是否会接触目标文本框 | 会，至少为了定位和插入结果 | 已证实 | 用户需先将光标置于文本框；Typeless 随后把结果插入该文本框。[首次听写](https://www.typeless.com/help/quickstart/first-dictation) |
| 是否会读取应用内文本 | 会读取有限的相关文本 | 已证实 | 隐私政策称会处理应用名称和应用内相关文本，以生成上下文感知结果。[Privacy Policy](https://www.typeless.com/privacy) |
| Accessibility 权限用途 | 官方公开用途是插入文本及响应 Fn 快捷键 | 已证实 | FAQ 和安装说明均这样描述。[FAQ](https://www.typeless.com/help/faqs)；[安装说明](https://www.typeless.com/help/installation-and-setup) |
| Accessibility 是否也用于检测后续编辑 | 可能，但未公开说明 | 可推断，未证实 | 自动词典功能需要获得纠正结果，而 Accessibility 具备接触目标文本框的条件；但 Typeless 从未公开声明具体监听方式。 |
| 是否持续监控所有输入框 | 无证据 | 未知 | 官方只说“when you use Typeless”时处理有限上下文，没有给出后台持续监控范围、开始/结束时点或事件类型。[Data Controls](https://www.typeless.com/data-controls) |
| 是否记录用户每次按键 | 无证据 | 未知 | 权限说明、隐私政策和发布说明均未声明按键记录。不能由 Accessibility 权限本身推出存在键盘记录。 |
| 是否扫描 History 来自动加词 | 无证据 | 未知 | History 被说明为本机保存、供用户找回转录；官方未把它列为词典学习来源。[历史记录说明](https://www.typeless.com/help/troubleshooting/missing-transcript) |
| 是否从当前上下文直接抽取专有词 | 技术上可能，未公开 | 未知 | 官方承认使用相关文本作实时上下文，但只说明目的为上下文感知转录，没有说明会从中持久化词条。[Data Controls](https://www.typeless.com/data-controls) |
| 自动词条是否保存在本机 | 很可能，但没有明确的数据模型说明 | 可推断，未证实 | 官方宣称听写数据、编辑和屏幕上下文不在服务器保留；同时个人词典必须跨听写持久存在。官方没有明确说明词典的本机/云端存储和同步机制。 |
| 自动加入是否一次纠正即触发 | 文案看起来如此，但阈值未知 | 可推断，未证实 | “The next time you correct a word”暗示单次纠正即可加入，但没有公开去噪、置信度、延迟或例外规则。[macOS 发布说明](https://www.typeless.com/help/release-notes/macos) |

## 官方明确公开了什么

### 1. “说完后纠正”是自动词典信号

这是目前最关键的新证据。Typeless 在 2025-09-23 发布的 macOS v0.4.0 说明中，把功能命名为 **Auto-added dictionary**，并说明：用户在说完后纠正一个词，无论是同事姓名、项目代号还是品牌名，系统都会自动保存到个人词典。Windows 在 2026-01-28 的 v0.9.1 说明中使用了近似描述：系统会捕捉用户说完后纠正的词，使姓名、术语和偏好拼写在未来结果中正确出现。[macOS 发布说明](https://www.typeless.com/help/release-notes/macos)；[Windows 发布说明](https://www.typeless.com/help/release-notes/windows)

因此，先前只能确认“支持自动加入”、却无法确认候选来源的结论已经可以收窄：**至少在桌面端，用户对语音输出的后续纠正就是官方承认的自动词典来源。**

不过，“correct a word after speaking”仍是产品行为描述，不是技术协议。它没有回答：

- 纠正必须发生在 Typeless 刚插入的那段文本内，还是同一输入框任意位置都算；
- 是替换、删除后重输、粘贴，还是输入法候选选择都能触发；
- 纠正后立即加入，还是等待发送、失焦、下一次听写或其他提交信号；
- 系统如何排除普通改写、语法润色、撤销和误操作；
- 是否保存“错误形式 -> 正确形式”映射，还是只保存正确词；
- 是否存在长度、字符集、重复次数、语言或置信度门槛。

### 2. Typeless 已具备读取和写入目标文本框的产品能力

桌面端首次听写说明要求用户先点击任意文本框，然后 Typeless 将格式化结果直接插入其中。[首次听写](https://www.typeless.com/help/quickstart/first-dictation)

macOS FAQ 对 Accessibility 权限的公开解释包括两项：

- 把文本直接插入光标所在位置；
- 让用户通过 Fn 键在不切换应用的情况下触发听写。

[FAQ](https://www.typeless.com/help/faqs)

此外，Privacy Policy 和 Data Controls 都明确承认：使用 Typeless 时，会把语音与有限上下文一起处理；上下文例子包括当前应用以及其中的相关文本。[Privacy Policy](https://www.typeless.com/privacy)；[Data Controls](https://www.typeless.com/data-controls)

这足以证明 Typeless 并非只把一段文本“盲打”到目标应用，它具有定位文本框、插入文本及读取有限相关文本的能力。但这些资料仍不足以证明它在 Typeless 不活跃时持续订阅所有文本变化。

### 3. “零保留”指服务器保留，不等于设备上不保存个人学习状态

Data Controls 称听写数据（audio、transcripts、edits）不会被 Typeless 或第三方存储或用于模型训练；相关语音和上下文在云端实时处理，结果返回设备后即丢弃。[Data Controls](https://www.typeless.com/data-controls)

Privacy Policy 进一步明确，不在服务器保存语音录音、转录或屏幕上下文数据。[Privacy Policy](https://www.typeless.com/privacy)

这不能推出“Typeless 不保存自动词条”。个人词典本身就是用户期望持久存在的产品状态；历史记录也被官方明确说明为本机保存。[历史记录说明](https://www.typeless.com/help/troubleshooting/missing-transcript) 合理解释是：完整听写、编辑和上下文不在服务器长期保留，而必要的个人词典条目或个性化状态可能在本机持久化。但官方没有公开个人词典的存储位置、同步方式或数据结构，因此不能把这一解释当作已证实实现。

## 对“监测用户输入框”的判断

用户提出的假设可以拆成两个强度不同的命题：

| 命题 | 判断 | 原因 |
|---|---|---|
| Typeless 会检测用户对它刚生成文本所做的纠正 | **高度可信，且产品行为已获官方证实** | 发布说明直接把“说完后纠正词”描述为自动词典触发行为。 |
| Typeless 持续监控用户在所有目标应用输入框中的每一次编辑 | **未证实，当前证据不足** | 官方没有披露监听周期、范围、事件、按键采集或实现 API；检测纠正也可通过更窄的方式完成。 |

### 与现有证据最吻合的可能实现

以下只是实现推断，不是 Typeless 官方披露：

```text
Typeless 插入一段结果
        |
        v
本机短期保存：目标应用 / 输入框标识 / 插入范围 / 原始结果
        |
        v
用户在该范围附近进行编辑
        |
        v
读取修改后的局部文本，与刚插入结果做局部 diff
        |
        v
筛出类似姓名、术语、偏好拼写的短替换
        |
        v
写入个人词典，后续听写使用
```

它能解释“修改几个单词后自动加入”的用户观察，也符合“说完后纠正”的官方措辞。为了避免把普通编辑误判成词典，它很可能需要把观察范围限制在 Typeless 刚插入的文本附近，并设置时间窗、编辑规模或词形过滤条件；这些过滤规则均未公开。

### 同样符合官方描述的替代实现

1. **下一次唤起时比对。**Typeless 不持续监听；下次按 Fn 时读取当前相关文本，与本机保存的上次输出比对，再提取纠正词。
2. **只监听 Typeless 插入后的短时间窗。**插入完成后临时订阅焦点文本框变化，窗口结束即停止。
3. **记录局部键盘/文本事件。**Accessibility 或平台级输入事件可用于定位局部变更，但官方没有承认记录按键，不能据此下结论。
4. **应用或控件特定机制。**不同文本框可能通过不同方式读取修改后的值；跨平台功能也不要求 macOS 和 Windows 使用完全相同的底层机制。

因此，更严谨的表述是：

> Typeless 官方确认会从“语音输入后的用户纠正”自动学习个人词汇。它必然以某种方式获得纠正后的局部文本，但公开资料不足以证明它会持续监控所有输入框或记录每一次按键。

## History、Context、Edits 与 Personalization 的边界

| 数据/能力 | 官方说明 | 与自动词典的关系 |
|---|---|---|
| History | 转录历史保存在用户设备，并在用户选择的 Keep History 期限后自动删除。[历史记录说明](https://www.typeless.com/help/troubleshooting/missing-transcript) | 未公开称用于抽取个人词条。历史存在不等于词典学习扫描历史。 |
| Context Awareness | 使用 Typeless 时处理当前应用和其中的有限相关文本，云端实时处理后丢弃。[Data Controls](https://www.typeless.com/data-controls) | 证明具备读取局部文本的能力；未证明上下文中的词会被直接持久化到词典。 |
| Edits | Data Controls 把 edits 纳入不用于训练、服务器零保留的 Dictation Data；发布说明称说完后的纠正可进入个人词典。[Data Controls](https://www.typeless.com/data-controls)；[macOS 发布说明](https://www.typeless.com/help/release-notes/macos) | 这是目前与自动词典最直接关联的信号。完整编辑内容不在服务器保留，与本机保留精简词条并不矛盾。 |
| Personalization | 随使用学习正式/随意、简洁/详细等写作风格；可在 Settings > Personalization 关闭。[个性化说明](https://www.typeless.com/help/release-notes/macos/personalized-smarter) | 属于语气和表达风格学习，不应与姓名、术语、偏好拼写的个人词典混为一谈。 |
| Voluntary feedback | 用户主动提交反馈并明确同意时，Typeless 可收集伪匿名文本或纠正用于改善全体用户模型。[Privacy Policy](https://www.typeless.com/privacy) | 这是全局模型改进的可选数据流，不等同于自动维护本人的个人词典。 |

## 仍未公开的关键实现参数

- 自动纠正检测发生在编辑当下，还是下一次调用 Typeless 时；
- 是否依赖 Accessibility 文本变化通知、轮询、焦点变化、剪贴板或键盘事件；
- 对哪个文本范围做 diff，以及跟踪窗口持续多久；
- 单次纠正是否足够，还是需要重复出现；
- 如何区分拼写纠正、普通改写、语法调整和整句重写；
- 自动加入的是正确词，还是同时保留错误词到正确词的映射；
- 词典条目的本地存储、云同步、加密、跨设备迁移与删除语义；
- 是否存在独立的“自动词典学习”开关；
- 自动词条是否标记来源，以及用户能否撤销刚加入的词条；
- 不同平台是否使用相同触发规则。

## 可验证实现假设的黑盒实验

若需要进一步判断 Typeless 是“持续监听”还是“下一次唤起时比对”，可以在不逆向软件的前提下做以下可重复实验：

| 实验 | 操作 | 观察意义 |
|---|---|---|
| 即时性 | 听写错误词，手动纠正后立刻打开 Dictionary，不再次唤起 Typeless | 若立即出现，支持编辑时监听或短期轮询；若下次唤起后才出现，支持延迟比对。 |
| 时间窗 | 分别在 5 秒、1 分钟、10 分钟后纠正同一类错误 | 可估计是否存在短期跟踪窗口。 |
| 范围 | 在刚插入段落内纠正、同一文本框旧段落纠正、其他文本框纠正 | 可判断它是否只跟踪刚插入范围。 |
| 编辑方式 | 分别使用逐字键入、粘贴、输入法候选、撤销后重输 | 可区分按键监听与最终文本 diff。 |
| 失焦/退出 | 插入后切换应用或退出 Typeless，再返回纠正 | 可判断跟踪是否依赖当前会话或后台进程。 |
| Accessibility | 关闭 Accessibility 后尝试可行的复制粘贴流程 | 若自动学习停止，只能说明该权限是必要条件之一，仍不能证明具体使用了哪种事件。 |

实验时应使用虚构姓名和无敏感信息的文本，并记录 Typeless 版本、macOS 版本、目标应用、编辑方式、等待时间和词典出现时点。产品行为可能随版本变化，结果只代表被测版本。

## 最终判断

用户的观察不是偶然猜测：Typeless 官方已经明确承认“语音输入后纠正词语 -> 自动加入/升级个人词典”这条产品链路。因此，**把人工纠正视为高可信学习信号是有竞品事实支撑的。**

但“通过 Accessibility 持续监测所有输入框”比现有证据多走了一步。Accessibility 的公开用途是插入文本和快捷键；官方同时承认会处理有限相关文本，却没有说明后续编辑检测的技术机制。现阶段最准确的结论是：

> Typeless 很可能在本机对刚插入文本的后续局部修改进行关联或延迟比对，但是否采用持续输入框监听、监听多久、监听多大范围以及怎样过滤误修改，均未公开。

## 一手来源

- [Typeless macOS app release notes](https://www.typeless.com/help/release-notes/macos)，查询日期：2026-07-15。
- [Typeless Windows app release notes](https://www.typeless.com/help/release-notes/windows)，查询日期：2026-07-15。
- [Typeless 官网](https://www.typeless.com/)，查询日期：2026-07-15。
- [Typeless Data Controls](https://www.typeless.com/data-controls)，页面最后更新：2026-01-09；查询日期：2026-07-15。
- [Typeless Privacy Policy](https://www.typeless.com/privacy)，页面最后更新：2026-03-13；查询日期：2026-07-15。
- [Typeless FAQ](https://www.typeless.com/help/faqs)，查询日期：2026-07-15。
- [Typeless Installation and setup](https://www.typeless.com/help/installation-and-setup)，查询日期：2026-07-15。
- [Typeless: Your first dictation](https://www.typeless.com/help/quickstart/first-dictation)，查询日期：2026-07-15。
- [Typeless: Where did my transcript go?](https://www.typeless.com/help/troubleshooting/missing-transcript)，查询日期：2026-07-15。
- [Typeless macOS v0.9.0: Personalized. Smarter.](https://www.typeless.com/help/release-notes/macos/personalized-smarter)，查询日期：2026-07-15。
