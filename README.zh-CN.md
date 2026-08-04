<p align="center">
  <img src="apps/desktop/icons/saymore-mark-3d-136.png" width="96" alt="Saymore 标志">
</p>

<h1 align="center">Saymore</h1>

<p align="center">
  <strong>自然表达，随处输入。</strong><br>
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
  · <a href="#目前可以做什么">功能</a>
  · <a href="docs/README.md">文档</a>
  · <a href="CONTRIBUTING.md">参与贡献</a>
</p>

Saymore 可以直接在当前光标位置把语音转换成文字，让你在编辑器、浏览器、聊天应用、
终端或其他常用输入框中完成听写。按下全局快捷键，自然说话，Saymore 会完成识别、
可选文本精炼，并将结果写入当前应用，无需切换到单独的编辑界面。

它是一款使用 Rust 和 [Slint](https://slint.dev/) 构建的原生桌面应用，明确区分
语音识别、文本精炼、数据存储和平台集成边界。Provider 由用户选择，Saymore 不要求
登录托管账户，也不依赖 Saymore 云端后端。

> **项目状态：** Saymore 已可在 macOS 与 Windows 上安装使用，但仍在积极开发中。
> 最新 Release 的功能可能稍晚于仓库当前状态。

## 目前可以做什么

| 领域             | 当前能力                                                                                                                     |
| ---------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| **跨应用听写**   | 使用可配置的全局快捷键开始和结束录音，再把文字写入当前可编辑控件。macOS 默认使用 Right Command，Windows 默认使用 Right Alt。 |
| **语音识别**     | 可使用 macOS 听写、火山引擎或自定义 OpenAI 兼容语音接口；具体可用项取决于操作系统与 Provider 配置。                          |
| **可选文本精炼** | 使用 SenseNova 或 DeepSeek 保守处理填充词、标点、自我修正和文本结构。精炼结果异常或服务不可用时，会自动回退到识别文本。      |
| **个人词典**     | 添加、编辑、删除、搜索和筛选标准写法，也可从 CSV 导入。在支持观察的文本控件中，Saymore 还能根据多次纠正学习词语。            |
| **本地历史**     | 搜索、查看、复制、删除或清空加密历史，并配置保留期限。原始音频不会落盘保存。                                                 |
| **桌面体验**     | 提供首次引导、权限检测、麦克风选择、多快捷键、登录时启动、托盘控制、主题、中文/英文界面、更新检查和隐私安全的诊断报告导出。  |

当文字投递无法确认时，Saymore 会保留最终转录结果供用户恢复，而不会静默丢弃。

## 工作原理

```text
全局快捷键
    -> 在内存中录制语音
    -> 使用已配置的 ASR Provider 识别
    -> 执行确定性的本地清理
    -> 可选地使用已配置的 LLM Provider 精炼
    -> 规范化词典中已确认的标准写法
    -> 在当前光标位置写入文字
```

文本精炼被严格限制在改善转录内容：它不会回答问题、编造事实，也不会把听写变成
聊天机器人。如果 Provider 失败，或返回结果不符合 Saymore 的安全约束，处理链会
自动回退到上一阶段的安全文本。

## 隐私边界

- 音频仅在识别过程中保留于内存，不写入本地历史。
- 使用云端 ASR 时，音频会发送给用户配置的 Provider；启用云端精炼时，转录文本和
  本次相关的已确认词典条目会发送给所选 LLM Provider。
- 本地历史经过加密，密钥保存在操作系统凭据存储中；用户可以关闭历史或修改保留
  期限。
- 密码框等敏感控件会被特殊处理，不进入历史，也不参与纠正学习。
- 诊断数据仅保存在本地，只记录白名单内的事件标识，不记录转录、API Key、设备名、
  文件路径或原始错误详情。
- Saymore 不读取屏幕上下文、不生成回复，也不会自动发送消息。

[产品主路线](docs/product/open-source-voice-input-wayfinder.md)定义了完整的数据、
Provider 与功能边界。

## 下载

| 平台          | 安装包                                                                                                                                         |
| ------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| **macOS 12+** | [GitHub Releases](https://github.com/PraxisGrove/Saymore/releases/latest) 提供经过签名和公证、同时支持 Apple Silicon 与 Intel Mac 的通用 DMG。 |
| **Windows**   | [GitHub Releases](https://github.com/PraxisGrove/Saymore/releases/latest) 提供面向当前用户安装的 NSIS 安装程序。                               |

每个直接发布版本都附带 `SHA256SUMS`，可用于校验下载文件。Mac App Store 版本仍在
规划中，目前尚未提供。

安装后：

1. 完成首次引导并授予麦克风权限。macOS 还需要辅助功能权限，用于全局快捷键和
   跨应用文字写入。
2. 在“模型”页面选择并测试一个语音识别 Provider。
3. 聚焦任意可编辑输入框，使用已配置的快捷键开始和结束听写；录音期间可按 Escape
   取消。

## 本地开发

生产桌面应用使用 Rust 与 Slint，不依赖 Node.js、WebView 或 Web 前端。

在 macOS 上启动具有稳定签名身份、可持续刷新的开发预览：

```bash
./scripts/dev-preview.sh
```

在 Windows 上使用 Cargo 构建桌面应用：

```powershell
cargo build -p saymore-desktop
```

workspace 包含四个清晰的职责边界：

| 路径           | 职责                                     |
| -------------- | ---------------------------------------- |
| `crates/app`   | 业务类型、不变量、用例与端口 trait       |
| `crates/infra` | 文件系统、数据库、网络、音频和平台适配器 |
| `apps/desktop` | Slint UI、依赖装配与进程生命周期         |
| `crates/xtask` | 仓库维护与打包自动化                     |

```text
desktop -> app
desktop -> infra -> app
```

前置依赖、预览与打包流程、完整质量门禁见[开发指南](docs/development.md)。crate
所有权与平台边界见[架构文档](docs/architecture.md)。

## 文档

- [产品方向与范围](docs/product/open-source-voice-input-wayfinder.md)
- [架构](docs/architecture.md)
- [开发](docs/development.md)
- [测试](docs/testing.md)与[审查](docs/review.md)
- [发布](docs/releasing.md)
- [技术栈](docs/technology-stack.md)

[文档索引](docs/README.md)包含完整的产品、工程、ADR 和调研文档。

## 参与贡献

欢迎提交 Issue、参与设计讨论、反馈文档问题和提供可复现的缺陷报告。开始实现前请先
阅读 [CONTRIBUTING.md](CONTRIBUTING.md)。外部贡献者 CLA 流程尚未上线，因此当前
代码贡献需要先与维护者沟通。

## 许可证

Saymore 是**源码可用（source-available）**项目，不属于 OSI 认可的开源软件。
项目使用 [PolyForm Shield License 1.0.0](LICENSE)。个人使用、组织内部使用以及
其他不构成竞争的用途均被允许。提供与 Saymore 竞争的产品或服务，需要另行取得
维护者的商业许可。第三方资产继续适用各自的许可证。
