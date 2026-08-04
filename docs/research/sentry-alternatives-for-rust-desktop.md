# Sentry 类产品与 Rust 桌面应用适配调研

调研日期：2026-08-04

范围：面向 Saymore（local-first、Rust、Slint、macOS/Windows
桌面应用）的错误监控、
崩溃诊断和性能遥测。只引用厂商官方文档、官方源码仓库/API，以及 CNCF 的官方调查。
GitHub、GitLab 和 crates.io
数字只用于判断生态可见度与维护活跃度，不代表市场份额或 活跃用户数。

## 结论先行

- **若要最快获得 Rust panic、错误上下文和版本聚合，首选 Sentry SaaS。**它有官方
  Rust SDK，直接支持 error、panic、`tracing`、OpenTelemetry 和 backtrace；但
  Rust SDK 在 1.0 前仍不承诺严格 semver。SaaS 免费 Developer
  套餐为单用户，包含错误监控和 tracing；Team 当前从 26
  美元/月起。[Rust SDK](https://github.com/getsentry/sentry-rust)
  [价格](https://sentry.io/pricing/)
- **若隐私和自托管优先，GlitchTip 是最接近的替代。**它直接使用 Sentry Rust SDK，
  能自动捕获 panic 并采集 transaction；官方表述是“力求”兼容 Sentry API，并非完整
  等价。托管免费层为每月 1,000 events；Small 为 15 美元/月、100,000
  events；也可用 Docker Compose
  自托管。[Rust 接入](https://glitchtip.com/sdkdocs/rust)
  [兼容边界](https://glitchtip.com/documentation/integrations/)
  [价格](https://glitchtip.com/pricing/)
- **OpenTelemetry 适合做长期的 vendor-neutral 日志、指标、链路层，不是 Sentry
  的直接 替代。**它不提供 issue 聚合、崩溃收件箱或 minidump 分析 UI，必须再接
  Collector 和 存储/查询后端。Saymore 可先保留本地诊断，再按需增加 OTel
  exporter；若首要目标是
  桌面崩溃定位，仍需错误/崩溃产品。[OTel 定义](https://opentelemetry.io/docs/what-is-opentelemetry/)
  [Rust 实现](https://github.com/open-telemetry/opentelemetry-rust)
- **不建议首版选 Bugsnag、Rollbar、Raygun、Datadog 或 New Relic。**Bugsnag 和
  Raygun 没有官方 Rust SDK；Rollbar 的官方 Rust 仓库已停止主动维护；Datadog 的
  Rust tracing 需要手工 OpenTelemetry instrumentation，New Relic 也主要通过 OTel
  span 进入 Errors Inbox。它们适合已有相应全栈监控合同的团队，不是 Saymore
  当前最短、最可靠的桌面 崩溃接入路径。

## 候选对比

| 产品                     | Rust 与桌面适配                                                                                                                                                | 性能/全栈能力                                      | 隐私与部署                                                                                                                   | 当前公开价格信号                                                                            | Saymore 判断                                       |
| ------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- | -------------------------------------------------- |
| Sentry                   | 官方 Rust SDK；error、panic、backtrace、`tracing`、OTel；SDK 未到 1.0。Sentry 后端支持 native crash/minidump，但 Rust panic 捕获不等于进程级 native crash 捕获 | errors + tracing；产品另含 logs/metrics 等         | SaaS；官方 self-host 套件定位为低流量部署和 PoC，运维明显重于 GlitchTip                                                      | Developer $0/单用户；Team $26/月                                                            | **首选 SaaS**；需单独验证 hard crash/minidump 路径 |
| GlitchTip                | 复用 Sentry Rust SDK；自动捕获 panic；Sentry API 兼容存在边界                                                                                                  | errors + performance + uptime + logs               | MIT 开源；Docker Compose + PostgreSQL，可完全控制数据位置                                                                    | Hosted 1,000 events/月免费；Small $15/月、100K events                                       | **首选自托管/隐私方案**                            |
| Highlight.io             | 官方 `highlightio` Rust SDK 可手工上报 error、stack trace 和 OTel span；没有官方证据表明能捕获桌面 native crash/minidump                                       | 强项是 Web session replay、前后端关联、logs/traces | 可 hobby self-host，但官方不建议超过 10K sessions 或 50K errors/月；企业自托管从 $3,000/月起，hobby 部署默认发送匿名部署遥测 | 免费层 $0、500 sessions、最多 15 seats；PAYG 从 $50/月起                                    | Rust 可用，但产品重心不匹配 Slint 桌面             |
| Honeybadger              | 官方主语言文档没有 Rust SDK；需用通用 Reporting API 自建 panic/error 上报                                                                                      | errors + logging/performance + uptime              | SaaS；single-tenant/self-host 仅 Enterprise 洽谈                                                                             | 免费：5K errors/月、50 MB/day、单用户；Team $26/月                                          | 低成本手工上报可用，但不是 native crash 方案       |
| Bugsnag                  | 官方支持 macOS、C/C++ minidump 等桌面能力，但官方平台列表没有 Rust；需 C/C++ 桥接或自建 Reporting API 适配                                                     | errors + performance，移动/桌面成熟                | SaaS；on-prem 仅 Enterprise                                                                                                  | 免费：7.5K events、1M spans/月、单用户                                                      | 原生能力好，Rust 接入成本抵消优势                  |
| Rollbar                  | 官方 `rollbar-rust` 仓库明确自 2024-05 起停止主动维护，README 称其并非完整 SDK、也未发布到 crates.io                                                           | 以 error grouping、deploy/version tracking 为核心  | SaaS                                                                                                                         | 免费：5K occurrences + 1K replay sessions；Essentials 从 $9/月                              | **不建议新接**                                     |
| Raygun                   | 官方语言列表有 C++、Apple、.NET 等，但没有 Rust；桌面/移动 crash 产品成熟                                                                                      | Crash Reporting、RUM、APM 分开计费                 | SaaS                                                                                                                         | 无长期免费层；14 天试用；Crash Reporting 低于 50K errors 为 $40/月（年付）或 $60/月（月付） | 成本与 Rust 适配均不占优                           |
| Datadog Error Tracking   | 官方 Rust tracing 基于 `datadog-opentelemetry`，且明确没有自动 instrumentation；不是 Rust 桌面 crash SDK                                                       | 全栈 APM、logs、RUM、infra 强                      | SaaS                                                                                                                         | Error Tracking 低于 50K errors 为 $25/月（年约）或 $36 on-demand                            | 仅在团队已使用 Datadog 时合理                      |
| New Relic Errors Inbox   | 可从 OTel error span 聚合错误；没有专用 Rust 桌面 crash SDK 证据                                                                                               | APM、logs、infra、Errors Inbox 一体                | SaaS                                                                                                                         | 免费层：1 个 full-platform user、100 GB/月 ingest；超额 Original data $0.40/GB              | 免费量大，但接入和诊断语义偏服务端                 |
| OpenTelemetry + 自建后端 | 官方 Rust logs/metrics/traces/OTLP；不自动捕获 native crash                                                                                                    | 可接 Collector、Prometheus、Jaeger、Grafana 等     | 数据路径最可控，但要自负部署、告警、保留和升级成本                                                                           | 软件免费，基础设施与运维不免费                                                              | **适合作为遥测底座，不单独承担 crash triage**      |

表中功能与价格来源：[BugSnag 平台列表](https://docs.bugsnag.com/platforms/)、
[BugSnag 价格](https://www.bugsnag.com/pricing/)、[BugSnag C/C++ minidump](https://docs.bugsnag.com/platforms/cpp-minidump/)、
[Rollbar Rust 仓库](https://github.com/rollbar/rollbar-rust)、
[Rollbar 价格](https://rollbar.com/pricing)、[Raygun 语言列表](https://raygun.com/documentation/language-guides/)、
[Raygun 价格](https://raygun.com/pricing)、[Datadog Rust tracing](https://docs.datadoghq.com/tracing/trace_collection/dd_libraries/rust/)、
[Datadog 价格](https://www.datadoghq.com/pricing/)、[New Relic Errors Inbox](https://docs.newrelic.com/docs/errors-inbox/getting-started/)、
[New Relic 价格](https://newrelic.com/pricing)、[Honeybadger 价格](https://www.honeybadger.io/plans/)、
[Highlight Rust SDK](https://www.highlight.io/docs/sdk/rust)、[Highlight 价格](https://www.highlight.io/pricing)、
[Highlight hobby self-host](https://highlight.io/docs/general/company/open-source/hosting/self-host-hobby)、
[Sentry self-host](https://github.com/getsentry/self-hosted)。

## 开发者目前在用什么

没有找到覆盖这些厂商、方法透明且能直接回答“错误监控市场份额”的近期独立调查，
因此不能严谨地给出 Sentry、Bugsnag、Rollbar 等的占比。可确认的信号是：

- Stack Overflow 2025 Developer Survey 的 AI agent 子样本中，2,689 名回答该题的
  开发者有 **43% 使用 Grafana + Prometheus、32% 使用 Sentry** 做 agent
  可观测性。这说明传统监控工具仍有较强采用度，但该题只覆盖 AI agent
  开发者，不能当作通用
  错误监控市场份额。[调查结果](https://survey.stackoverflow.co/2025/ai#3-ai-agents)
- 截至 2026-08-04，官方仓库 API 显示 Sentry 主仓库约 **44.5K stars**，Sentry
  Rust SDK **748 stars / 130 contributors / 90 releases**；`sentry` crate 累计约
  **45.8M** 下载。这个量级和持续发布说明 Sentry 在 Rust
  错误上报中有成熟可见度，但 crate 下载会包含
  CI、缓存和间接依赖，不能换算成用户数。
- Highlight 官方仓库约 **9.4K stars / 85 contributors / 57 releases**；GlitchTip
  官方 GitLab backend 为 **356 stars / 182 forks / 36
  contributors**；两者均持续活跃，但分别更偏 Web 全栈体验和轻量
  Sentry-compatible 自托管。
- OpenTelemetry Rust 官方仓库约 **2.7K stars / 302 contributors / 43
  releases**， `opentelemetry` crate 累计约 **233M** 下载。CNCF 2025 Annual
  Survey 在 395–502 个 cloud-native 受访组织中给出 **49% 已用于生产、26%
  正在评估**；这能说明 OTel 已是主流遥测标准，但调查样本偏
  cloud-native，不能直接外推到桌面应用。

计数来源：[Sentry GitHub API](https://api.github.com/repos/getsentry/sentry)、
[Sentry Rust GitHub API](https://api.github.com/repos/getsentry/sentry-rust)、
[crates.io `sentry`](https://crates.io/api/v1/crates/sentry)、
[Highlight GitHub API](https://api.github.com/repos/highlight/highlight)、
[GlitchTip GitLab API](https://gitlab.com/api/v4/projects/glitchtip%2Fglitchtip-backend)、
[OpenTelemetry Rust GitHub API](https://api.github.com/repos/open-telemetry/opentelemetry-rust)、
[crates.io `opentelemetry`](https://crates.io/api/v1/crates/opentelemetry)、
[CNCF 2025 Annual Survey](https://www.cncf.io/wp-content/uploads/2026/01/CNCF_Annual_Survey_Report_final.pdf)。

## Saymore 的建议落地顺序

1. 保留现有本地、隐私安全的诊断事件和用户主动导出报告，线上遥测必须是**明确
   opt-in**，默认关闭，并能随时撤回。
2. 做一个小型 adapter spike，同时把 Sentry SaaS 与 GlitchTip 指向不同 DSN
   验证：Rust panic、handled error、release/version、离线退出时
   flush、Windows/macOS 符号化。
3. 事件白名单只允许错误码、组件、版本、OS/架构和匿名安装 ID。禁止上传识别文本、
   润色文本、音频、剪贴板、输入目标内容、API key、用户文件路径和完整
   URL；发送前做 二次 scrub，并限制 breadcrumb。
4. 单独制造 abort、segfault/access violation、OOM 和 UI/worker-thread
   panic。只有在 macOS 与 Windows 都获得可符号化 stack
   后，才能声称“崩溃监控”；普通 Rust panic 上报不足以覆盖这些 hard crash。
5. 若未来需要性能分析，先用 `tracing` 建立内部 span，再决定直送
   Sentry/GlitchTip，或经 OpenTelemetry Collector
   输出。不要为了“以后可能换厂商”先部署完整自建栈。

当前推荐：**早期产品选 Sentry SaaS；有明确数据驻留/自托管要求时选 GlitchTip；把
OpenTelemetry 留作后续性能和结构化遥测层。**
