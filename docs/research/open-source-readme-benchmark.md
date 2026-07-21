# 优秀开源项目 README 基准调研

> 调研日期：2026-07-21 范围：Rust、桌面、local-first 与生产力工具相关的 6 个官方
> GitHub README。只引用项目维护方的仓库，不以 star 数或二手文章代替内容分析。

## 结论先行

成熟 README 的“高级感”主要来自清楚的信息优先级，而不是更多徽章、居中的 HTML
或更长的功能列表：

1. 第一屏先回答“这是什么、为何值得、下一步去哪”，用户安装先于源码构建。
2. 桌面产品用清晰的真实 UI 截图证明产品已经存在；框架型项目才可以不放截图。
3. README 是入口和导航，不是把全部开发文档复制到仓库首页。
4. 徽章只保留会影响判断的动态信号，例如构建状态、许可证和发布状态。
5. 可信度来自具体的支持平台、数据边界、许可证、CI 与可验证界面，不来自“fast /
   secure / powerful”等形容词堆叠。

对 Saymore 最合适的组合是：用 Zed/Lapce 的克制骨架，用一张真实主界面图和
少量工作流图证明产品价值，借 LocalSend 的下载与平台信息表达，并把复杂的架构、
质量门禁和分平台开发说明继续留在 `docs/`。

## 样本与具体模式

| 项目                                                                    | README 的实际顺序                                                                                                         | 值得借鉴                                                                                                      | 不应照搬                                                                                              |
| ----------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| [Zed](https://github.com/zed-industries/zed/blob/main/README.md)        | 名称与两个徽章 → 一句定位 → 安装 → 分平台开发文档 → 贡献 → 许可与赞助                                                     | 首屏极短，安装入口非常靠前；分平台细节全部链接到文档。它证明成熟感不等于 README 很长                          | 作为已有强品牌和完整官网的产品，它可以省略截图与功能说明；Saymore 当前还不能依赖同等品牌认知          |
| [RustDesk](https://github.com/rustdesk/rustdesk/blob/master/README.md)  | 品牌横幅/页内导航 → 多语言 → 滥用警告 → 社区 → 核心价值与截图 → 下载 → 依赖、构建、结构 → 更多截图                        | 高风险远程控制产品把安全边界前置；下载、FAQ 和 nightly 入口醒目；明确说明可自托管                             | 多语言链接挤占首屏，后半段有很长的构建墙，并仍夹有 deprecated Sciter 路径；信息层级和维护状态显得混杂 |
| [LocalSend](https://github.com/localsend/localsend/blob/main/README.md) | 名称与 3 类状态徽章 → 社区/语言 → 一句定位 → 目录 → About → 截图 → 跨平台下载矩阵 → 兼容性/设置 → 原理 → 开发与排障       | “局域网、安全、无需互联网”一句话说清差异；截图、下载和平台约束都在源码构建之前；复杂发行渠道用矩阵呈现        | README 同时承担用户手册，长度较大；Saymore 不必在首页复制完整排障和每个平台的 maintainer build 命令   |
| [AppFlowy](https://github.com/AppFlowy-IO/AppFlowy/blob/main/README.md) | 品牌/品类定位/价值主张 → 徽章与社区 → 多张真实产品图 → 用户安装 → 技术栈 → 开发、roadmap、发布、贡献 → 使命               | 最产品化的样本：在安装之前用真实 Kanban、数据库、AI、模板和多端界面证明能力；同时把 OS 特定开发步骤链接到文档 | 连续长图和后部的长使命宣言会拉长页面；Saymore 应精选一张主图和 2–3 个确有区分度的局部图，而不是做图库 |
| [Tauri](https://github.com/tauri-apps/tauri/blob/dev/README.md)         | Logo 与项目信号徽章 → Introduction → 原理/架构链接 → Getting Started 的一条命令 → Features → Platforms → 贡献、组织与许可 | 对能力边界、平台、架构和许可证表达具体；只给一条最快起步命令，其余交给文档                                    | 徽章数量偏多，且框架 README 不需要产品截图；Saymore 是终端产品，不能用技术栈替代产品演示              |
| [Lapce](https://github.com/lapce/lapce/blob/master/README.md)           | 可点击品牌与 tagline → 3 个入口 → 技术差异 → 一张真实大图 → Features → Installation → 贡献/反馈 → 许可                    | 产品视觉与 Rust 技术可信度最平衡；下载、包管理器和源码构建分别链接，整体仍很短                                | 功能列表偏开发者工具语境；Saymore 应用“用户结果”组织能力，而不是按内部模块列功能                      |

这些项目并非每一段都同样优秀。尤其 Zed 的短、AppFlowy 的视觉密度、LocalSend
的操作手册深度和 RustDesk 的构建细节彼此矛盾；正确做法是按 Saymore
当前阶段组合，而不是选择一个 README 逐段仿写。

## 对 Saymore 当前 README 的判断

当前 `README.md` 准确记录了 Rust/Slint 技术栈、crate 边界、质量门禁、文档入口和
许可证，这是很好的维护者资料；但首页的主要读者看不到产品本身：

- 开头有一句定位，却没有 logo、真实界面、核心工作流或明确的用户价值证明。
- `Structure` 和 `Tooling Policy`
  比安装/试用更早，首页首先服务了贡献者而非用户。
- 完整质量命令在 `Tooling Policy`、`Development`、`Tests` 和可选 `just` 段落中
  多次出现，增加扫描成本。
- 缺少清晰的产品状态、支持平台/系统要求、下载或当前可运行方式，以及 roadmap/
  issue/贡献入口的优先级。
- 当前许可证是 PolyForm Shield，README 已准确说明它是 **source-available，非
  OSI-approved open source**。任何新版都必须保留这一口径，不能为了“开源感”使用
  `open source` 徽章或文案。

## 建议的目标结构

建议按以下顺序重写，而不是仅做颜色和徽章调整：

1. **品牌首屏**：Saymore 名称/logo；一句可验证定位；一行核心价值；最多 3–4 个
   徽章（build、platform/release、license、Slint）；明确的
   Download/Build、Docs、Contribute 入口。
2. **真实产品图**：一张无遮挡的桌面主界面图。若当前 UI 尚未稳定，应标注
   `work in progress`，不要用与实物不符的概念图。
3. **Why Saymore**：用 3–5 个用户结果说明全局触发、本地优先、provider-agnostic、
   文本整理与投递；避免把 crate 或实现库当卖点。
4. **How it works**：用一条紧凑流程表达
   `trigger → record → recognize → optionally refine → insert`，并明确每一步的数据
   是否留在本机、何时可能调用用户配置的 provider。
5. **Get Saymore / Project status**：按 macOS、Windows
   列出实际可用状态、系统要求、
   下载入口或当前的最短源码运行路径。尚未发布时应直说，而不是放无效按钮。
6. **Development**：只保留 prerequisites 和最短 happy path；完整质量门禁链接到
   `docs/development.md`，架构链接到 `docs/architecture.md`。
7. **Contributing / Roadmap / Community**：明确适合的新贡献者入口、issue
   规范和产品 roadmap；若社区渠道尚不存在，不创建空链接。
8. **License**：保留显眼、准确的 source-available 说明和商业许可边界。

## 内容与视觉约束

- 不做“徽章墙”。CI、许可证、发布/平台是有决策价值的信号；语言占比、编辑器、
  无实际入口的 social badge 不应占首屏。
- 不使用模糊、裁切过度或无法对应当前版本的截图。截图应显示真实录音/识别/投递
  状态，并用一致窗口尺寸、浅色或深色背景与简短 alt text。
- 不在 README 重复整套 Cargo gate。首页只需最短启动命令和“完整开发文档”链接。
- 不写尚未交付的能力为现在时。可以用明确的 `Current status` / `Planned` 区分。
- 不以“local-first”作为无法核验的口号；应说明音频、转录稿、provider 调用和最终
  存储的实际边界。
- 英文主 README 适合目前的代码与潜在贡献者范围；需要中文时，用独立、可维护的
  `README.zh-CN.md`，不要让长语言导航抢占第一屏。

## 推荐改写范围

第一轮只改 README 的信息架构和现有事实，不顺便承诺发行渠道、社区或未完成能力：

- 新增品牌首屏、项目状态、1 张真实截图和紧凑工作流；
- 将架构、完整 gate、测试与 workspace 约定压缩为文档导航；
- 增加清晰的安装/当前运行入口与贡献入口；
- 保留并强化 source-available 许可证口径；
- 逐个验证图片、徽章、锚点和外链，保证浅色/深色 GitHub 主题都可读。

这会让首页从“工程规则索引”变成“产品入口 + 可信的贡献者入口”，同时不牺牲当前
仓库已经建立的架构与质量文档。

## 一手来源

- [Zed 官方 README](https://github.com/zed-industries/zed/blob/main/README.md)
- [RustDesk 官方 README](https://github.com/rustdesk/rustdesk/blob/master/README.md)
- [LocalSend 官方 README](https://github.com/localsend/localsend/blob/main/README.md)
- [AppFlowy 官方 README](https://github.com/AppFlowy-IO/AppFlowy/blob/main/README.md)
- [Tauri 官方 README](https://github.com/tauri-apps/tauri/blob/dev/README.md)
- [Lapce 官方 README](https://github.com/lapce/lapce/blob/master/README.md)
