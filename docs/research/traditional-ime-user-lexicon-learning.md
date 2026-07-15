# 传统输入法用户词库学习机制及其对 Saymore 的启示

- 查询日期：2026-07-15
- 范围：Rime/librime、AOSP LatinIME、Fcitx5/libime，以及 Apple、Microsoft 官方输入框架与用户词典文档
- 证据标准：只引用官方文档和官方源码；无法从公开资料确认的内容标为“工程推断”

## 结论摘要

传统输入法的核心学习对象通常不是一条无条件的“错误文本 -> 正确文本”替换规则，而是：

1. 用户在一次 composition 中输入了什么编码或按键序列；
2. 用户最后选择并 commit 了哪个候选；
3. 这个词及相邻词出现了多少次、最近是否出现；
4. 用户是否撤销自动纠正、手动选了别的候选或主动遗忘某个词。

这些信号用于加入可召回词条以及调整 unigram/bigram、候选词频和排序。确定性的文本替换属于另一类显式功能，例如 Apple Text Replacements；它由用户明确配置，并不等同于输入法的隐式学习。

因此，Saymore 可以借鉴传统输入法的“观察 -> 累计证据 -> 提高召回/排序 -> 可遗忘”闭环，但不应把观察到的 `before -> after` 直接执行为全局本地替换。对于 Saymore，`before` 最多是一个 observed variant（召回证据），`after` 才是 canonical term；是否在未来输出中使用 canonical term，应由识别/润色阶段结合本次语音和上下文判断，而不是由全局字符串替换器决定。

## 已证实的实现

| 系统 | 学习触发 | 学习数据 | 使用方式 | 衰减/删除/隐私 |
|---|---|---|---|---|
| AOSP LatinIME | 用户实际输入、手动候选选择、自动纠正及其撤销 | 词、前词上下文、有效性、次数、时间戳；支持二元组 | 用户历史词典参与建议和排序；撤销可取消历史学习 | 用户历史词典是本地统计；实现有 decaying dictionary；关闭个性化会删除历史词典；敏感/不适合纠正的字段不学习 |
| Rime/librime | composition 被确认后产生 commit；translator 可启用用户词典 | 用户选择的词条及其输入编码、commit 次数/词频 | 调整候选字词频与排序；用户词典可禁用 | 用户词典可部署、同步、导出或清理；具体衰减策略依配置和实现版本，不应假定为统一算法 |
| Fcitx5/libime | 候选已选中并准备 commit 时调用 `learn()` | 词、输入编码、unigram、bigram、句子历史、上下文 | 用户历史分数与静态语言模型分数组合，影响解码和预测 | 多级有界历史池造成随新输入发生的自然衰减；支持 forget、清空用户词典/历史；密码或敏感控件不学习 |
| Apple 日语输入源 | 用户完成假名到汉字转换 | 用户偏好的转换历史 | 常用转换在候选窗中提前 | 可重置转换历史；Apple 未公开内部频率或衰减公式 |
| Apple 文本替换/中日用户词典 | 用户显式添加 reading/replace 与 output/with | 明确的用户规则 | 确定性文本替换或按读音召回 | 可删除、导入导出，并可经 Apple Account 同步；这不是隐式学习 |
| Microsoft 日语 IME | 官方策略称存在 input history、self-tuned words 和 custom dictionary | 公开文档未披露内部权重结构 | 自动调优候选 | 用户可清空 input history/auto-tuning；企业策略可关闭 custom dictionary |

### AOSP LatinIME

`UserHistoryDictionary` 的类注释直接说明，它在本地收集用户输入词的统计，以及自动纠正撤销、手动候选选择等信号，从而随时间适应用户。当前接口保存 n-gram context、word、validity、count 和 timestamp；支持到 bigram（`SUPPORTED_NGRAM = 2`）。这说明其主要模型是带上下文的历史权重，而不是永久的错误词映射。[AOSP UserHistoryDictionary](https://android.googlesource.com/platform/packages/inputmethods/LatinIME/+/fa1e65cb3a5dcce6299a6dd067cee95720107307/java/src/com/android/inputmethod/latin/personalization/UserHistoryDictionary.java)

旧版但更易追踪的官方源码展示了具体闭环：commit 一个候选后加入 user history；若用户撤销自动纠正，则调用 `cancelAddingUserHistory(previousWord, committedWord)` 移除刚才的 bigram；手动选择候选是独立的 commit 类型。这是“正负反馈修正统计”，不是把 typed word 永久映射到 picked word。[AOSP LatinIME commit/undo 路径](https://android.googlesource.com/platform/packages/inputmethods/LatinIME/+/5d2556b93286f5f1d7d829b586b84a8b7ae55743/java/src/com/android/inputmethod/latin/LatinIME.java) [AOSP 旧版 UserHistoryDictionary](https://android.googlesource.com/platform/packages/inputmethods/LatinIME/+/7247bff/java/src/com/android/inputmethod/latin/UserHistoryDictionary.java)

新版设置代码在关闭 personalized dictionaries 时删除所有 user history dictionaries。另一个官方路径会在非 backspace 类型的“unlearn”事件中移除用户历史词条。[AOSP 个性化关闭行为](https://android.googlesource.com/platform/packages/inputmethods/LatinIME/+/5657746/java/src/com/android/inputmethod/latin/LatinIME.java) [AOSP unlearn 实现](https://android.googlesource.com/platform/packages/inputmethods/LatinIME/+/master/java/src/com/android/inputmethod/latin/DictionaryFacilitatorImpl.java)

### Fcitx5/libime

Fcitx5 Pinyin 在候选已经选中时，先检查 `PasswordOrSensitive`，非敏感目标才执行 `context.learn()`，随后才 `commitString(sentence)`。[Fcitx5 Pinyin commit 路径](https://github.com/fcitx/fcitx5-chinese-addons/blob/3969ce948c5db03cd167dcc61f55888dca28ee7e/im/pinyin/pinyin.cpp#L428-L447)

libime 的 `PinyinContext::learn()` 将已选词及其拼音编码加入 history，并把已有上下文传给 `addWithContext`。[libime Pinyin 学习](https://github.com/fcitx/libime/blob/7b638a433815ed7a29d9bcb8d59aed7366bd3b28/src/libime/pinyin/pinyincontext.cpp#L1089-L1118) `HistoryBigram` 对每个已提交句子增加 unigram 和相邻 bigram 频次；用户语言模型把历史分数与静态语言模型分数组合，而不是执行字符串替换。[libime HistoryBigram](https://github.com/fcitx/libime/blob/7b638a433815ed7a29d9bcb8d59aed7366bd3b28/src/libime/core/historybigram.cpp#L382-L418) [libime UserLanguageModel](https://github.com/fcitx/libime/blob/7b638a433815ed7a29d9bcb8d59aed7366bd3b28/src/libime/core/userlanguagemodel.cpp#L122-L140)

其衰减不是简单按日期乘一个系数，而是把新句子保存在多个容量为 128、8192、65536 的历史池中；较新记录位于权重更高的小池，被挤出的记录逐级进入更大的低权重池。因此旧行为会随着新 commit 自然降低影响。[libime 多级历史池](https://github.com/fcitx/libime/blob/7b638a433815ed7a29d9bcb8d59aed7366bd3b28/src/libime/core/historybigram.cpp#L519-L605)

Fcitx5 还提供候选遗忘：从 user dictionary 删除单词，并调用 history `forget`；也可分别清空用户词典或用户词典加全部历史。词典与历史保存为独立的 `user.dict` 和 `user.history` 文件。[Fcitx5 forget/clear/save](https://github.com/fcitx/fcitx5-chinese-addons/blob/3969ce948c5db03cd167dcc61f55888dca28ee7e/im/pinyin/pinyin.cpp#L1625-L1640)

### Rime/librime

Rime 的 translator 通过 `enable_user_dict` 决定是否使用用户词典；用户确认候选后，用户词典记录对应输入编码与候选并调整字词频，从而影响以后相同或相关编码下的候选排序。Rime 官方配置说明也把“用户词典”与静态词典区分，并提供用户资料同步机制。[Rime 配置指南](https://github.com/rime/home/wiki/Configuration) [librime 官方仓库](https://github.com/rime/librime)

公开资料支持“提交候选后调整用户词频/候选排序”这一结论，但没有支持“Rime 默认建立任意错误文本到正确文本的确定性全局替换”。具体频率公式和衰减应以所选 librime 版本源码为准，不能从配置项名称外推。

### Apple 与 Microsoft

Apple 官方说明：日语输入源在假名转汉字时学习用户偏好的转换并优先显示，用户可以 Reset 清除 conversion history。[Apple 日语输入设置](https://support.apple.com/guide/japanese-input-method/change-japanese-settings-jpim662a12b9/mac)

Apple 另有显式 Text Replacements，用户亲自填写 replace/with；中文、日文输入源会把这类规则包含在 user dictionary。该功能是明确授权的确定性替换，不能用来证明系统会把所有手动纠正自动变成规则。[Apple Text Replacements](https://support.apple.com/guide/mac-help/replace-text-punctuation-documents-mac-mh35735/mac) [Apple 中文用户词典](https://support.apple.com/guide/chinese-input-method/edit-your-chinese-user-dictionary-cim165f83982/mac)

Microsoft 官方企业策略说明，日语 IME 的 custom dictionary 中存在 self-tuned words，并提供“Clear input history”和“Clear auto-tuning information”；这证实其有可关闭、可清理的自动学习，但未公开 unigram/bigram 或衰减公式。[Microsoft Japanese IME policy](https://learn.microsoft.com/en-us/windows/client-management/mdm/policy-csp-admx-eaime)

## 为什么传统输入法比跨应用听写拥有更强的学习信号

输入法位于 composition 生命周期内部。它知道：

- 原始按键/读音编码；
- 当前仍在变化的 marked/composing text；
- 展示过哪些候选；
- 用户点选了哪个候选；
- 何时 commit；
- 用户是否立即撤销了由输入法产生的自动纠正。

Apple InputMethodKit 明确区分 current composition 与 commit：composition 通过 marked text 更新，`commitComposition` 结束会话。[Apple updateComposition](https://developer.apple.com/documentation/inputmethodkit/imkinputcontroller/updatecomposition%28%29) [Apple commitComposition](https://developer.apple.com/documentation/objectivec/nsobject-swift.class/commitcomposition%28_%3A%29)

Windows TSF 同样把 composition 定义为“仍在变化的临时输入状态”，并提供 start、update、end 事件和明确的文本范围。[Microsoft TSF Compositions](https://learn.microsoft.com/en-us/windows/win32/tsf/compositions)

Android `InputConnection` 则直接提供 `setComposingText`、`finishComposingText` 和 `commitText`；IME 可以读取光标前后文本，且知道自己提交的文本和位置。[Android 创建输入法](https://developer.android.com/develop/ui/views/touch-and-input/creating-input-method) [Android InputConnection](https://developer.android.com/reference/android/view/inputmethod/InputConnection)

Saymore 当前是“生成完整文本后，通过 AX 或剪贴板投递到另一个应用”，不拥有目标应用的 composition。投递后的编辑可能是：

- 纠正识别错误；
- 改变事实或语气；
- 删除不想发送的内容；
- 普通续写、粘贴或整句重写；
- 目标应用自身的自动纠正。

所以“同一控件中，Saymore 刚投递范围附近发生了局部修改”只是传统 IME commit/undo 信号的近似。Accessibility 或 UI Automation 能提高归因能力，但不能把歧义消除到输入法原生 composition 的程度。

## 对 Saymore 的设计约束

### 1. 本地词库不应是全局替换表

建议区分三个概念：

```text
canonical term: 用户希望系统认识和优先考虑的正式写法，例如 Saymore
observed variant: 某次识别后被用户改掉的表面形式，例如 cm
replacement rule: 无条件把 cm 改成 Saymore
```

前两者可以存储；第三者不应由隐式观察自动产生。即使 `cm -> Saymore` 出现多次，也可能存在用户确实想说字母 `CM` 的场景。传统输入法的对应做法是让 `Saymore` 获得更高候选权重，同时保留其他候选，而不是删除原候选。

### 2. 第一阶段只建立“用户词库识别”闭环

第一阶段不需要把屏幕上下文发送给 LLM，也不需要实现最终替换。建议完成：

1. 为每次成功投递建立短生命周期 insertion receipt；
2. 仅观察同一控件、同一局部范围和有限时间窗口；
3. 在敏感控件、Secure Input、不可读取范围时明确返回 unavailable；
4. 等待编辑稳定后做局部 diff；
5. 把“原表面形式、canonical term、次数、最近时间、独立投递次数、证据类型、置信度”写入候选数据；
6. 不保存完整输入框、窗口内容或周围 300--500 字正文；局部锚点只在内存中存在；
7. 重复证据提升候选置信度，长期未出现则降低权重；
8. 用户可确认、拒绝、删除和永久抑制；删除必须同时清理它对排序的影响；
9. 先用离线评估量化 precision，再决定是否允许高置信候选自动成为 confirmed term。

### 3. 将来的使用应是“受控召回”，不是“确定性替换”

第一阶段只负责把真实用户行为变成高质量的本地词库。未来使用 confirmed term 时，应优先：

- 把 canonical term 作为 ASR phrase/hotword 候选；
- 在 relevant-term 检索中，用 confirmed observed variant 帮助召回 canonical term；
- 把 canonical term 与“这是候选而非强制替换”的约束交给上下文感知的识别或润色阶段；
- 保留原始转录和最终输出的可追踪性，以便撤销和调试。

不要在最终输出后运行全局 `str.replace(observed_variant, canonical)`。只有用户显式创建的文本替换规则，才适合确定性执行。

## 已证实与工程推断的边界

### 已证实

- AOSP LatinIME 学习 typed words、manual picks、autocorrection cancellation、n-gram context、时间戳等信号。
- Fcitx5/libime 在候选 commit 路径学习 unigram、bigram、编码与上下文，并在敏感控件跳过学习。
- libime 通过多级有界历史池让旧行为随新输入降低影响，并支持 forget/clear。
- Apple 和 Microsoft 输入法均提供可重置/清理的转换或输入历史。
- 主流平台的 IME API 都提供明确的 composition/commit 生命周期。

### 工程推断

- Saymore 应把投递后局部纠正视为较强但非确定性的证据。
- 一次局部修改不足以形成全局映射；跨独立投递重复、修改范围稳定和低编辑距离可提高置信度。
- `observed variant` 最安全的用途是召回 canonical term，而不是本地替换。
- 对 Saymore 来说，基于新证据次数与最近时间的简单衰减足够作为第一版；无需复制 libime 的三级历史池。
- Accessibility/UI Automation 只能恢复“文本在哪里被改了”，无法完全恢复“为什么被改”。
