# Typeless macOS 文本投递实现逆向推断

调研日期：2026-07-14

## 结论

可以，而且现有证据已经足以把关键结论从“猜测”提高到较高置信度：**至少在本机安装的 Typeless macOS 2.0.1 中，普通文本投递的主路径是临时写入系统剪贴板、模拟 `Command+V`，随后恢复原剪贴板；不是通过 `AXUIElementSetAttributeValue` 直接修改目标输入框。**

Typeless 的 Accessibility 权限仍然重要，但静态证据更支持它主要用于定位焦点、读取选区和上下文、处理目标窗口兼容性，以及允许全局交互；不能再根据官网的 “insert text directly” 宣传语推断其底层一定是 AX 写值。Typeless 官方确实说 Accessibility 用于把文本插入光标位置，并称用户无需复制粘贴；这些是用户可见的产品描述，不是底层 API 承诺。[Typeless FAQ](https://www.typeless.com/help/faqs) [Installation and setup](https://www.typeless.com/help/installation-and-setup)

## 证据等级

本文区分三类结论：

| 等级 | 含义 |
|---|---|
| 已确认：官方公开资料 | Typeless 官网或帮助中心明确说明的产品行为 |
| 已确认：本机一方制品 | 对 Typeless 官方签名安装包进行只读静态检查得到的符号、依赖和调用序列 |
| 推断 | 根据调用序列解释设计意图；不是 Typeless 官方架构声明 |

本机样本信息：

- 应用：`/Applications/Typeless.app`
- Bundle ID：`now.typeless.desktop`
- 版本：`2.0.1`，构建号 `2.0.1.115`
- Team ID：`947QKAND4W`
- 主应用是 Electron；`Info.plist` 的 `NSPrincipalClass` 为 `AtomApplication`
- 原生输入 helper：`Contents/Resources/lib/input-helper/build/libInputHelper.dylib`
- helper SHA-256：`7c5aacac72268875e9a44fd3bf749864e0613163802476c96345e0a904ffca0a`

因此，本文关于二进制的结论只适用于该样本；Typeless 后续版本可能改变实现。

## 官方公开事实

Typeless 的快速入门要求用户先点击任意文本框并保留闪烁光标，结束口述后再把最终结果插入该文本框。[Your first dictation](https://www.typeless.com/help/quickstart/first-dictation)

Typeless 明确要求 macOS Accessibility 权限，并解释该权限用于把文本插入当前光标位置，同时支持不切换应用即可使用全局快捷键。[Typeless FAQ](https://www.typeless.com/help/faqs) [Installation and setup](https://www.typeless.com/help/installation-and-setup)

安装说明称用户不需要手动复制、粘贴或打断焦点。这只能确认用户体验，不足以区分内部采用剪贴板粘贴、AX 写值还是 Unicode 键盘事件。[Installation and setup](https://www.typeless.com/help/installation-and-setup)

macOS v0.4.1 的发布说明特别提到支持国际键盘布局。这个事实与本机 helper 根据当前键盘布局查找 `V` 键码的实现相吻合，但发布说明没有公开原因，不能单独作为实现证据。[Typeless macOS release notes](https://www.typeless.com/help/release-notes/macos)

## 本机 2.0.1 静态证据

### 普通文本投递路径

`libInputHelper.dylib` 导出了以下 Swift/C 入口或内部符号：

```text
insertText
performTextInsertion(with:)
savePasteboard()
restorePasteboard(from:)
simulatePasteCommand(completion:)
insertRichText
performRichTextInsertion(html:text:)
```

`performTextInsertion(with:)` 的反汇编调用序列明确显示：

```text
savePasteboard()
NSPasteboard.generalPasteboard
prepareForNewContentsWithOptions(1)
NSPasteboardItem.setString(..., NSPasteboardTypeString)
NSPasteboardItem.setData(..., "org.nspasteboard.TransientType")
generalPasteboard.writeObjects(...)
simulatePasteCommand(completion: restorePasteboard)
return true
```

选项值 `1` 对应 `NSPasteboard.ContentsOptions.currentHostOnly`；Apple 将它定义为剪贴板内容只在当前设备可用，不会提供给其他设备。[Apple: currentHostOnly](https://developer.apple.com/documentation/appkit/nspasteboard/contentsoptions/currenthostonly)

这说明 Typeless 不是简单覆盖纯文本后就不管，而是做了几层保护：

1. 保存原剪贴板的全部 item 和类型。
2. 临时写入待投递文本。
3. 把临时内容限制在当前 Mac，避免进入 Universal Clipboard 的跨设备路径。Apple 说明 general pasteboard 默认会参与 Universal Clipboard，因此这一选项有实际隐私价值。[Apple: NSPasteboard](https://developer.apple.com/documentation/appkit/nspasteboard)
4. 额外写入 `org.nspasteboard.TransientType`。这个字符串和写入行为可以确认；它对每个剪贴板管理器的实际效果不是 Apple 公共 API 合同，不能保证第三方工具一定忽略该内容。
5. 粘贴后约 100 ms 调用 completion，恢复之前保存的剪贴板。

### `Command+V` 的生成方式

`simulatePasteCommand` 的内部闭包会：

1. 用当前键盘输入源和键盘布局查找字符 `V` 对应的虚拟键码；找不到时回退到固定键码。
2. 创建 `V` 的 key-down 和 key-up `CGEvent`。
3. 给两个事件设置 Command 修饰键。
4. 通过 `CGEventPost` 投递按下和抬起事件。
5. 延时约 100 ms 执行恢复剪贴板的 completion。

Apple 将 `CGEvent` 定义为低层 Quartz 输入事件，并提供创建键盘事件与投递事件流的 API。[Apple: CGEvent](https://developer.apple.com/documentation/coregraphics/cgevent) [Apple: post(tap:)](https://developer.apple.com/documentation/coregraphics/cgevent/post%28tap%3A%29)

按当前布局解析 `V` 也解释了为什么 Typeless 曾单独修复国际键盘布局兼容性：它不是把 Unicode 文本逐字模拟成键盘输入，而是要确保“粘贴命令”的物理键码在不同布局下仍正确。这一因果关系是推断，不是发布说明直接披露的原因。

### Accessibility 的实际角色

`libInputHelper.dylib` 导入了：

```text
AXUIElementCreateSystemWide
AXUIElementCopyAttributeValue
AXUIElementCopyMultipleAttributeValues
AXUIElementCopyParameterizedAttributeValue
```

但该 input helper **没有导入** `AXUIElementSetAttributeValue`。它还导出了 `getSelectedText()` 和 `getSelectedTextBySimulateCopyAsync(...)`，后者名称和调用表明选区读取也有模拟复制的后备路径。

另一个 `libContextHelper.dylib` 导入 AX 的读取 API，并导出获取当前应用、聚焦元素、可见文本和相关内容的函数。它确实导入 `AXUIElementSetAttributeValue`，但静态符号显示对应的公开入口是 `setFocusedWindowEnhancedUserInterface()`；这不能证明它用 AX 写入最终口述文本。

Apple 分别把 `AXUIElementCopyAttributeValue` 定义为读取可访问性属性，把 `AXUIElementSetAttributeValue` 定义为写入属性。[Apple: AXUIElementCopyAttributeValue](https://developer.apple.com/documentation/applicationservices/1462085-axuielementcopyattributevalue) [Apple: AXUIElementSetAttributeValue](https://developer.apple.com/documentation/applicationservices/1460434-axuielementsetattributevalue)

综合来看，2.0.1 更可能采用以下职责划分：

```text
Accessibility
  -> 找到前台应用和聚焦元素
  -> 读取选区、输入框信息和周边上下文
  -> 必要时调整目标窗口的 enhanced user interface 属性

NSPasteboard + CGEvent
  -> 投递最终纯文本或富文本
  -> 触发目标应用自身的粘贴行为
  -> 延时恢复用户原剪贴板
```

这与官方“Accessibility 让 Typeless 能在光标处插入文本”的说法不冲突：Accessibility 是整个跨应用能力的必要组成，但最终写入动作可以由剪贴板和键盘事件完成。

### 成功语义

`performTextInsertion(with:)` 在成功写入剪贴板并安排模拟粘贴后立即返回 `true`。静态路径中没有看到它在返回前读取目标控件或光标来验证文本已经出现。

因此，Typeless 2.0.1 的这个 helper 更像返回“粘贴动作已发出”，而不是“目标输入框已验证包含文本”。上层是否另有异步验证逻辑，本次静态检查没有证明；不能据此断言 Typeless 整体完全没有投递验证。

## 高置信度实现图

```text
识别与精炼得到最终文本
  -> 保持目标应用/输入框为键盘焦点
  -> 保存 general pasteboard 的全部内容
  -> 以 currentHostOnly 方式准备临时剪贴板
  -> 写入纯文本和 transient 类型标记
  -> 根据当前键盘布局找到 V 键码
  -> 发送 Command+V 的 key-down / key-up
  -> 约 100 ms 后恢复原剪贴板
  -> helper 报告“已安排投递”
```

对于“光标仍在闪，但 AX 暂时拿不到聚焦控件”的场景，这套策略仍可能成功，因为 `Command+V` 由前台应用的实际键盘焦点处理，并不要求投递时取得一个可写 AX 文本元素。

## 对 Saymore 的启示

Typeless 的现实选择表明，剪贴板粘贴并不是勉强的末级补丁，而可以成为成熟跨应用语音工具的主投递路径。其价值在于让目标应用自己执行原生 Paste，从而覆盖浏览器、Electron、自绘编辑器和 AX 实现不完整的控件。

但不建议不加保护地照搬“一律覆盖剪贴板再粘贴”。如果 Saymore 采用同类主路径，至少应保留这些工程措施：

1. 保存并恢复全部剪贴板 item 与类型，而不只是一个纯文本字符串。
2. 使用 `currentHostOnly`，避免临时口述内容进入 Universal Clipboard。
3. 在临时 item 上加入 transient 标记，但不要把它当作所有剪贴板管理器都会遵守的安全保证。
4. 根据当前键盘布局解析 `V`，避免国际布局下固定虚拟键码失效。
5. 粘贴后留出短暂消费窗口再恢复剪贴板。
6. 明确区分“已发送粘贴事件”和“已验证写入”；如果 AX 可观察，则继续验证选区/光标变化。
7. 处理用户在 100 ms 窗口内主动复制内容的竞争，避免恢复动作覆盖用户的新剪贴板。Typeless 2.0.1 是否检查这类竞争，本次静态检查尚未确认。

Saymore 当前的“AX 直接写入优先、剪贴板兜底”并非错误，但 Typeless 的实现提供了一个值得认真考虑的替代排序：

```text
默认：受保护的临时剪贴板 + Command+V
辅助：AX 用于目标识别、上下文、选区和投递后验证
例外：密码框、Secure Input、前台应用变化时拒绝投递
```

这种排序通常更看重兼容性；现有 Saymore 排序更看重不触碰剪贴板和可验证性。最终选择应由真实应用兼容矩阵和隐私产品承诺决定，而不是由“Accessibility 听起来更原生”决定。

## 尚未确认

- Electron 上层是否针对特定应用选择不同投递路径。
- 普通文本路径是否存在未触发的 AX 直接写入实现；当前 input helper 没有相应导入，但不能排除其他动态加载模块。
- Typeless 是否在更高层验证投递结果、重试或显示恢复卡片。
- 恢复剪贴板前是否检查 ownership/change count，避免覆盖用户并发复制；导出符号和已检查调用序列不足以确认。
- Secure Input、密码框和前台应用切换时的完整拒绝策略。
- `org.nspasteboard.TransientType` 在各类剪贴板管理器中的实际遵循程度。
- Windows 版是否采用相同策略；本文只分析 macOS 2.0.1。

## 复核方法

本次只做只读静态检查，没有修改、注入或绕过 Typeless：

```bash
plutil -p /Applications/Typeless.app/Contents/Info.plist
codesign -dvvv /Applications/Typeless.app
nm -gU .../libInputHelper.dylib | swift demangle
nm -u .../libInputHelper.dylib
strings -a .../libInputHelper.dylib
lldb -b -o 'target create .../libInputHelper.dylib' \
  -o 'disassemble -n <Swift symbol>'
```

二进制升级后应先重新记录版本与 SHA-256，再复核符号和调用序列，不能把本文结论永久外推。

## 一手来源

- [Typeless FAQ](https://www.typeless.com/help/faqs)
- [Typeless installation and setup](https://www.typeless.com/help/installation-and-setup)
- [Typeless: Your first dictation](https://www.typeless.com/help/quickstart/first-dictation)
- [Typeless macOS release notes](https://www.typeless.com/help/release-notes/macos)
- [Apple: NSPasteboard](https://developer.apple.com/documentation/appkit/nspasteboard)
- [Apple: currentHostOnly](https://developer.apple.com/documentation/appkit/nspasteboard/contentsoptions/currenthostonly)
- [Apple: CGEvent](https://developer.apple.com/documentation/coregraphics/cgevent)
- [Apple: CGEvent.post(tap:)](https://developer.apple.com/documentation/coregraphics/cgevent/post%28tap%3A%29)
- [Apple: AXUIElementCopyAttributeValue](https://developer.apple.com/documentation/applicationservices/1462085-axuielementcopyattributevalue)
- [Apple: AXUIElementSetAttributeValue](https://developer.apple.com/documentation/applicationservices/1460434-axuielementsetattributevalue)
