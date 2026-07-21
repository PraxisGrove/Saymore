<p align="center">
  <img src="apps/desktop/icons/saymore-mark-3d-136.png" width="96" alt="Saymore 标志">
</p>

<h1 align="center">Saymore</h1>

<p align="center">
  面向 macOS 与 Windows 的本地优先语音输入工具。
</p>

<p align="center">
  <a href="README.md">English</a> | <a href="README.zh-CN.md">简体中文</a>
</p>

<p align="center">
  <a href="https://github.com/PraxisGrove/Saymore/actions/workflows/ci.yaml"><img src="https://github.com/PraxisGrove/Saymore/actions/workflows/ci.yaml/badge.svg" alt="CI 状态"></a>
  <a href="https://github.com/PraxisGrove/Saymore/releases/latest"><img src="https://img.shields.io/github/v/release/PraxisGrove/Saymore?display_name=tag" alt="最新版本"></a>
  <a href="https://github.com/PraxisGrove/Saymore/releases/latest"><img src="https://img.shields.io/badge/platform-macOS%20%7C%20Windows-4b5563" alt="支持平台：macOS 与 Windows"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-PolyForm%20Shield%201.0.0-d97706" alt="PolyForm Shield 1.0.0 许可证"></a>
</p>

<p align="center">
  <a href="https://github.com/PraxisGrove/Saymore/releases/latest"><strong>下载</strong></a>
  · <a href="docs/README.md">文档</a>
  · <a href="CONTRIBUTING.md">参与贡献</a>
</p>

无需切换到单独的编辑器，Saymore 可以直接在当前光标位置把语音转换成文字。通过
全局快捷键开始听写，自然说话，Saymore 会完成识别、可选文本精炼，并把最终结果
写入你正在使用的应用。

Saymore 使用 Rust 和 [Slint](https://slint.dev/) 构建，并明确区分语音识别、文本
精炼和存储边界，不把产品绑定到单一 Provider。

## 为什么选择 Saymore

- **在任何输入位置工作。** 在编辑器、浏览器、聊天应用、终端和其他桌面文本框中
  使用同一套听写流程。
- **本地优先。** 可以选择本地语音识别，让语音留在设备上；本地历史经过加密，并可
  配置保留期限。
- **Provider 可替换。** 自由选择本地或云端 ASR，并配置可选的 LLM 文本精炼，不
  依赖单一厂商。
- **忠于原意的文本精炼。** 整理填充词、标点和结构，但不会把听写变成聊天机器人，
  也不会凭空补充内容。
- **结果不会静默丢失。** 投递失败时保留可恢复文本，不会丢弃用户刚刚说过的内容。

## 工作原理

```text
全局触发器
    -> 录制语音
    -> 在本地或通过已配置的 ASR Provider 识别
    -> 执行安全的本地清理
    -> 可选地通过已配置的 LLM Provider 精炼
    -> 规范化已确认词语的标准写法
    -> 在当前光标位置写入最终文本
```

本地识别可以让音频处理留在设备上。选择云端 ASR 时，音频会发送到对应 Provider；
启用云端精炼时，转录文本会发送到已配置的 LLM Provider。Saymore
既不读取屏幕上下文，也不生成回复，并且不会自动发送消息。完整的数据与功能边界见
[产品主路线](docs/product/open-source-voice-input-wayfinder.md)。

## 下载与当前状态

Saymore 正在积极开发中。macOS 和 Windows 应用已经共享主要听写流程和绝大多数用户
功能，各平台特有的系统集成由对应的原生实现负责。

| 平台      | 分发方式                                                                                      |
| --------- | --------------------------------------------------------------------------------------------- |
| Windows   | 安装程序通过 [GitHub Releases](https://github.com/PraxisGrove/Saymore/releases/latest) 分发。 |
| macOS 12+ | 直接下载版本通过 GitHub Releases 分发；完成商店提交和发布流程后，计划同时上架 Mac App Store。 |

直接发布的安装包附带用于验证下载文件的校验和。

## 本地开发

生产桌面应用使用 Rust 与 Slint，不依赖 Node.js 或 Web 前端。

在 macOS 上启动可持续刷新的开发预览：

```bash
./scripts/dev-preview.sh
```

在 Windows 上使用 Cargo 构建桌面应用：

```powershell
cargo build -p saymore-desktop
```

前置依赖、预览行为、打包方式和完整质量门禁见[开发指南](docs/development.md)。workspace
遵循以下依赖方向：

```text
desktop -> app
desktop -> infra -> app
```

crate 所有权与平台边界见[架构文档](docs/architecture.md)。

## 文档

- [产品方向与范围](docs/product/open-source-voice-input-wayfinder.md)
- [架构](docs/architecture.md)
- [开发](docs/development.md)
- [测试](docs/testing.md)
- [发布](docs/releasing.md)
- [技术栈](docs/technology-stack.md)

[文档索引](docs/README.md)包含完整的产品、工程、ADR 和调研文档。

## 参与贡献

欢迎提交 Issue、参与设计讨论、反馈文档问题和提供可复现的缺陷报告。开始实现前请先
阅读 [CONTRIBUTING.md](CONTRIBUTING.md)。外部贡献者 CLA 流程尚未上线，因此当前
代码贡献需要先与维护者沟通。

## 许可证

Saymore 是**源码可用（source-available）**项目，不属于 OSI
认可的开源软件。项目使用
[PolyForm Shield License 1.0.0](LICENSE)。个人使用、组织内部使用以及其他不构成竞争
的用途均被允许。提供与 Saymore 竞争的产品或服务，需要另行取得维护者的商业许可。
第三方资产继续适用各自的许可证。
