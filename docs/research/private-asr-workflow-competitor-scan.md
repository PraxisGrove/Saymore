# 本地 ASR 工作流竞品扫描

> 调研日期：2026-07-13  
> 范围：私密录音与访谈工作台、垂直行业结构化语音记录、本地/私密会议产品  
> 方法：只采用产品官网、官方文档/隐私与安全页面、官方开源仓库等一手资料。价格是调研日官网公开价，不含税，后续可能变化。

## 结论先行

这三个方向都有成熟竞品，但竞争状态并不相同：

1. **访谈场景存在明显的“两段式断层”**：noScribe、MacWhisper、Vibe、Buzz 已把本地转写、说话人和校对做得相当完整；Dovetail、Condens 则把标签、证据片段、跨项目分析和团队知识库做得很深，但依赖云端。真正把“端侧转写 + 本地研究资料库 + 跨访谈证据分析”做成一体化产品的强势产品仍少。
2. **本地会议不是空白市场**：Meetily、anarlog（原 Hyprnote）已经正面占据开源、本地转写、无机器人参会的位置；MacWhisper 也覆盖自动会议录制。若 Saymore 只做“本地录音 + 转写 + 摘要”，差异不足。
3. **垂直行业的付费价值高，但护城河不在 ASR**：医疗、法律、施工/现场服务已经有大量“语音到结构化记录”产品。领先者卖的是嵌入业务系统、行业模板、术语准确率、审计与合规，而不是一个通用录音按钮。
4. **“不让机器人入会”不等于“本地”**：Granola、Krisp 等桌面型产品虽无需会议机器人，但其官方资料表明数据或转写会进入云端。应把“录制入口”“ASR 推理”“LLM 总结”“最终存储”四层分别核验。
5. Saymore 更可信的机会不是泛化的“隐私优先”，而是：**可核验的全链路本地处理、摘要到原声的证据回溯、中文/中英混说、项目级术语与跨录音分析，以及可替换的 ASR/LLM 运行时。**

## 部署口径

| 标签 | 本报告定义 |
| --- | --- |
| 端侧/离线 | ASR 在用户设备执行；核心流程可断网运行；音频无需离开设备 |
| 自托管/私有基础设施 | 服务运行在客户控制的服务器、VPC 或内网；不等于单机离线 |
| 私有云/区域云 | 仍由厂商或其子处理商托管，但提供数据驻留、专属区域或合规控制 |
| 普通云 SaaS | 音频、文本或二者上传至厂商/子处理商完成处理或存储 |

## 一、私密录音与访谈工作台

### 竞品矩阵

| 产品 | 直接度 | 核心工作流与用户 | 部署与隐私事实 | 公开价格 | 对 Saymore 的含义 |
| --- | --- | --- | --- | --- | --- |
| [noScribe](https://github.com/kaixxx/noScribe) | **直接** | 为定性社会研究和记者访谈设计；文件/批量转写、约 60 种语言、Pyannote 说话人区分、重叠语音/停顿标记、带音频跟随的校对编辑器 | **真正端侧/离线**；官方明确“完全在本机运行，无云端”；Windows/macOS/Linux，GPL-3.0 | 免费开源 | 最接近“私密访谈转写工作台”的直接对手。其弱项是项目知识库、跨访谈主题分析、团队协作和现代产品体验，而不是转写本身 |
| [MacWhisper](https://www.macwhisper.com/) | **直接/跨会议** | 访谈、播客、课程、文件批处理；说话人识别、编辑/搜索、字幕/文档导出、会议自动检测与系统音频录制、AI 摘要/聊天 | **本地 ASR**；官网与[隐私政策](https://www.macwhisper.com/legal/privacy-policy)称音频和文本不离开设备；但用户连接 OpenAI、Anthropic 等云模型时，下游 AI 处理不再是纯本地，也支持 Ollama/LM Studio | 免费版；Pro **€64 一次买断**、含终身更新（调研日官网） | 在 macOS 上几乎覆盖“本地转写工具”的完整功能面。Saymore 不能仅靠模型选择、批量转写、会议录音或导出竞争 |
| [Vibe](https://github.com/thewh1teagle/vibe) | 邻近（转写工具） | 文件/批量/麦克风/系统音频转写，字幕与多格式导出，说话人区分；Claude 摘要或 Ollama 本地分析；跨平台 | **真正离线 ASR**；官方仓库称数据不离开设备。使用 Claude 摘要时文本会交给云服务 | 免费开源 | 免费、跨平台、功能面广，压低“本地 Whisper GUI”的付费空间；但没有研究项目、证据编码与跨访谈综合 |
| [Buzz](https://github.com/chidiwilliams/buzz) | 邻近（转写工具） | 文件/YouTube/实时麦克风、语音分离、说话人识别、搜索与音频联动、监控文件夹、CLI 和插件 | **离线转写**；官方仓库明确在个人电脑离线运行；MIT、跨平台 | 免费开源 | 进一步说明“本地转写 + 查看器 + 自动化接口”本身已商品化 |
| [Dovetail](https://docs.dovetail.com/help/projects/transcribe-and-translate) | **直接（研究全流程）** | 用户研究/客户洞察；上传或现场录音、多人识别、实时高亮/评论/标签/脱敏、摘要、证据和跨项目知识库；支持自定义词表 | **普通云 SaaS**；官方明确由 Amazon Transcribe 和 AssemblyAI 转写，录音结束后音频自动上传；[安全文档](https://docs.dovetail.com/help/security-information)称其为 cloud-based 平台 | Free **$0**；Enterprise 询价（调研日官网） | 在研究分析和组织协作上是标杆，但云端处理敏感录音正是本地产品的切入点 |
| [Condens](https://condens.io/automated-transcription/) | **直接（研究全流程）** | 研究录音批量转写、时间戳校对、说话人、标签到音视频片段、highlight reel、跨项目全局标签、参与者库与利益相关方门户 | **区域云 SaaS**；[托管文档](https://condens.io/help/security-and-privacy/data-privacy/data-hosting-and-server-locations)允许选美国或欧盟区域，数据在所选区域存储和处理，不是本地/离线 | Lite **€15/月**或 €165/年；Business **€500/月**（年付 €6,000）；Enterprise 询价；官网列转写时长不限 | 功能上最能说明“访谈工作台”应超越转写。Saymore 的机会是把类似证据工作流做成本地单机/自托管，而非复制其云端协作套件 |

### 竞争判断

本赛道不是没有竞品，而是竞品集中在两个端点：

```text
本地、私密、擅长生成可靠转录稿        云端、协作、擅长从多份研究中提炼知识
noScribe / MacWhisper / Vibe / Buzz  →  Dovetail / Condens
```

Saymore 若选择此方向，产品边界应落在两端之间尚未解决好的部分：

- 每个主题、结论和引用都保留到原声时间戳的双向链接，而非只生成不可核查的摘要。
- 本地项目库支持跨访谈搜索、编码、主题聚类和证据集合；默认不建立厂商云账户。
- 中文人名、品牌名、行业术语与中英混说的项目级词表，能重新转写并追踪修改来源。
- 支持“本地单人版 → 团队自托管版”的迁移路径；协作不是默认把敏感音频上传厂商云。
- 对标对象应是 noScribe + Condens 的组合，而不是再做一个 Whisper 文件转换器。

## 二、垂直行业的结构化语音记录

### 竞品矩阵

| 产品 | 行业/直接度 | 从语音到业务结果 | 部署与隐私事实 | 公开价格 | 对 Saymore 的含义 |
| --- | --- | --- | --- | --- | --- |
| [voize](https://www.voize.ai/us) | 护理/养老，**直接** | 护理人员床旁自然口述，系统结构化护理记录并写回 EHR；官方列出 VIVENDI、Medifox Dan 等集成 | 移动端垂直 SaaS，官方称可[离线使用](https://www.voize.ai/us/preise)且符合 GDPR；但“离线可用”没有证明 ASR/LLM 全部端侧运行，不应归为已证实的全本地产品 | 按被照护人数授权，金额询价 | 很强地验证了“少填护理记录”的价值；Saymore 若做类似场景，必须提供同等级系统集成和合规能力 |
| [Raken](https://help.rakenapp.com/en/articles/14478739-how-to-create-a-daily-report-in-the-raken-mobile-app) | 建筑施工日报，**直接** | 移动端用 voice-to-text 免手输入 daily report、work log 和 note，再生成 [AI 日报摘要](https://help.rakenapp.com/en/articles/14502801-ai-daily-report-summaries-in-raken) | **云 SaaS/移动端**；官方未承诺端侧模型、自托管或私有云 | 询价 | “口述到既有日报字段”已经是成熟功能；本地 ASR 需要叠加离线工地、中文术语和现有表单写回才有差异 |
| [OpenSpace Field Notes](https://www.openspace.ai/products/field/field-notes/) | 施工现场，直接/邻近 | 巡视时用 [AI Voice Notes](https://www.openspace.ai/blog/openspace-ai-voice-notes/) 口述，笔记与 360 度现场图像及位置上下文绑定，沉淀为现场证据 | **云 SaaS**；有官方 [Trust Center](https://security.openspace.ai/)，未公开端侧推理或自托管承诺 | 询价 | 说明语音必须和现场位置、图像、时间关联；只有文字结构化不够形成现场工作流壁垒 |
| [Abridge](https://www.abridge.com/product) | 临床环境记录，**直接** | 医患对话生成可计费临床笔记并写入 Epic 等 EHR；强调 evidence-based note generation | **企业云 SaaS**；[隐私政策](https://www.abridge.com/privacy)说明其作为 Business Associate 按 BAA 代表医疗机构处理含 PHI 的 Customer Data；未公开离线/端侧/自托管承诺 | 企业询价 | 临床笔记的竞争核心是 EHR 集成、证据关联、合规和临床安全，不是模型是否能转写 |
| [Nabla Copilot](https://www.nabla.com/) | 临床环境记录，**直接** | 环境听诊生成结构化临床笔记并进入临床工作流 | **明确云 SaaS**；[安全页](https://www.nabla.com/security)说明托管于 Google Cloud，并列 SOC 2 Type II、ISO 27001 | 试用/企业询价，未见可核验公开金额 | 云部署给本地方案留下隐私差异，但医疗集成和验证成本仍远高于通用软件 |
| [BigHand Digital Dictation](https://www.bighand.com/en-us/our-solutions/digital-dictation-and-speech-recognition/) | 法律文书，直接/邻近 | 律师通过桌面/手机/平板口述，自动转写后进入可配置的秘书/文书工作流，可排序、追踪并保留审计轨迹 | 多端加 **Cloud**；官网未证明全本地或自托管 | 询价 | 法律用户购买的是可分派、可追踪、可审计的文书生产流程；单机转录稿只是入口 |

### 竞争判断

垂直语音产品的典型价值链是：

```text
行业现场语音 → 术语识别 → 结构化字段/模板 → 人工确认 → 写回业务系统 → 审计留痕
```

因此“部署一个本地 ASR”只解决第二步的一部分。真正决定购买的通常是：

- 是否能生成该行业已经使用的文档，而不是一篇通用摘要；
- 是否能写回 EHR、CRM、工单、巡检或项目管理系统；
- 谁对错误负责，如何确认、修订和追踪；
- 是否满足录音同意、数据保留、访问控制和行业合规；
- 在噪声、口音、专有名词、药名/设备名下是否可靠。

对 Saymore 最务实的进入方式，是选一个低监管但离线价值明确的窄场景，例如设备巡检、售后维修或现场验收，先把一个固定表单闭环做透。医疗虽然客单价高，但现有产品成熟，EHR 集成、临床安全和销售门槛也最高。

## 三、本地/私密会议产品

### 真正本地与自托管产品

| 产品 | 直接度 | 核心工作流 | 部署与隐私事实 | 公开价格 | 对 Saymore 的含义 |
| --- | --- | --- | --- | --- | --- |
| [Meetily](https://github.com/Zackriya-Solutions/meetily) | **直接** | 同时采集麦克风与系统音频、实时转写、导入旧录音、摘要与导出；Community 跨平台，Pro 增加模板和高级导出 | **真正端侧/离线**；Whisper/Parakeet 本地转写、录音和转录稿本地存储；摘要可用 Ollama，也可选云 API；Pro/Enterprise 提供自托管选项 | Community 免费 MIT；[Pro](https://meetily.ai/pro/) **$10/用户/月，按年付**（官网标注常规价 $25），14 天试用 | 与“本地 AI 会议纪要”正面重叠，且开源、Rust/Tauri、跨平台。基础会议摘要不足以形成 Saymore 差异 |
| [anarlog（原 Hyprnote）](https://github.com/fastrepl/anarlog) | **直接** | 自动捕获会议、端侧转写，用户笔记与转录结合，Markdown 落盘；BYO LLM，支持 Ollama/LM Studio 等 | **真正端侧、本地优先、可自托管**；官方称音频不离机、无云后端、无账户和追踪，数据为本地 `.md`；MIT | 免费开源 | 已占据“开源 Granola 替代品”定位。Saymore 需要在证据回溯、模型运行时、中文和项目记忆上明显更深 |
| [MacWhisper](https://www.macwhisper.com/meeting-recording) | **直接（macOS）** | 自动检测 Zoom/Teams/Webex/Skype/Discord 等会议，不用机器人，后台录制、转写和总结 | **本地录制和本地 ASR**，会后可离线访问；连接云 LLM 时摘要可能离机 | 免费；Pro €64 一次买断 | macOS 单机市场的强直接对手；其一次买断价格也限制纯会议录音工具的定价空间 |
| [screenpipe](https://github.com/screenpipe/screenpipe) | 邻近（工作记忆） | 24/7 捕获屏幕、系统音频和麦克风，端侧 Whisper 转写、说话人、时间线、全文/语义搜索、会议总结 Pipe 与本地 API | **本地优先且可离线**；默认 SQLite 本地存储，支持本地模型；当前为 source-available，商业使用需看许可证 | [官网](https://screenpi.pe/pricing)称付费计划 **$25/月起** | 把会议放进“连续个人记忆”而非独立文档。证明跨会议检索有价值，也暴露持续录制的同意、权限与存储风险 |

### 容易被误认为“本地”的云产品

| 产品 | 为什么看起来私密 | 官方资料实际表明 | 公开价格 |
| --- | --- | --- | --- |
| [Granola](https://www.granola.ai/) | 桌面端直接听电脑音频，不派机器人入会；会后不保留原始录音 | [隐私政策](https://docs.granola.ai/help-center/policies/privacy-policy)称个人数据存于美国 AWS，并使用音频转写处理商；原始录音在生成转录后删除，但转录稿和用户数据仍是云端数据。属于**云 SaaS**，不是本地 ASR | Basic $0；Business **$14/用户/月**；Enterprise **$35/用户/月** |
| [Krisp](https://krisp.ai/ai-meeting-assistant/) | 客户端降噪、不用会议机器人，并提供端侧英语转写 | 官方会议助手页面明确：英语可在设备端转写，另外 15 种语言使用服务端转写；[隐私政策](https://krisp.ai/privacy-policy/)还列出会议录音/转录处理、美国存储及子处理商。因此属于**混合部署**，不能把英语端侧能力外推为所有语言和最终存储全本地 | 7 天免费试用；官网列个人档 **$8/月（年付）或 $16 月付**，团队档 **$15/月（年付）或 $30 月付** |

### 竞争判断

“本地会议纪要”已不是蓝海。最低可用功能——无机器人、系统音频采集、本地转写、摘要、Markdown 导出——已有多个免费或低价实现。Saymore 若进入，至少需要把竞争单位从“单次会议文档”升级为“可信的本地决策档案”：

- 决策、分歧、行动项与原声时间戳一一绑定；点击结论可播放证据。
- 跨会议追踪某项决策为何改变、谁承诺了什么、哪些行动项逾期。
- ASR、说话人、摘要模型可替换，并清楚显示每一步是否离机。
- 会议级数据策略：排除应用/设备、暂停、同意提示、保留周期、彻底删除与可审计导出。
- 中文会议术语库和中英混说准确率，而不是泛化追求“支持 100 种语言”。

## 横向定位图

| 产品群 | 本地 ASR | 本地最终存储 | 说话人/校对 | 跨材料分析 | 行业结构化/系统写回 |
| --- | :---: | :---: | :---: | :---: | :---: |
| noScribe / MacWhisper / Vibe / Buzz | 强 | 强 | 中到强 | 弱 | 弱 |
| Dovetail / Condens | 否 | 否 | 强 | 强 | 研究行业内强 |
| Meetily / anarlog | 强 | 强 | 中 | 中 | 弱 |
| Granola / Krisp | Granola 否；Krisp 英语端侧、其他语言服务端 | 未承诺全本地 | 强 | 中到强 | 弱 |
| 医疗/法律/现场垂直 SaaS | 多数否 | 多数否 | 场景化 | 场景化 | **强** |

## 对 Saymore 的产品建议

### 更值得验证的楔子

**优先级 1：本地访谈证据工作台。** 不是挑战 MacWhisper 的转写体验，而是为研究者提供 noScribe 尚无、Condens 必须上云的那一半：项目库、跨访谈编码、结论到原声、完全本地分析。

**优先级 2：窄行业的离线结构化记录。** 先选一个具体表单和用户角色，例如设备巡检员说完后生成现有巡检单，并要求每个 AI 填充字段可回听、可确认。没有行业渠道前，不建议从医疗切入。

**优先级 3：会议作为同一底座的 Capture 模式。** 保留会议录制能力，但不把“AI 会议纪要”当独立定位；让会议与访谈共享证据库、术语、搜索和模型运行时。

### 需要避免的伪差异

- “数据不用于训练”不等于数据不上传；Saymore 应显示逐阶段数据流。
- “不用机器人入会”不等于端侧推理。
- “支持本地模型”不等于默认全本地；云模型连接要有显著状态和内容预览。
- “开源”本身不能替代工作流价值；Meetily、anarlog、noScribe、Vibe、Buzz 都已开源。
- 单纯增加 ASR 模型数量很难形成用户可感知优势，除非能自动按硬件/语言选择并以真实录音验证效果。

## 主要一手来源

### 访谈与通用本地转写

- [noScribe 官方仓库与 README](https://github.com/kaixxx/noScribe)
- [MacWhisper 官网、功能与价格](https://www.macwhisper.com/)
- [MacWhisper 隐私政策](https://www.macwhisper.com/legal/privacy-policy)
- [Vibe 官方仓库](https://github.com/thewh1teagle/vibe)
- [Buzz 官方仓库](https://github.com/chidiwilliams/buzz)
- [Dovetail 转写与翻译文档](https://docs.dovetail.com/help/projects/transcribe-and-translate)
- [Dovetail 安全文档](https://docs.dovetail.com/help/security-information)
- [Dovetail 定价](https://dovetail.com/pricing/)
- [Condens 自动转写文档](https://condens.io/help/using-condens/entering-data/automated-transcription)
- [Condens 托管地区说明](https://condens.io/help/security-and-privacy/data-privacy/data-hosting-and-server-locations)
- [Condens 定价](https://condens.io/pricing/)

### 会议与个人记忆

- [Meetily 官方仓库](https://github.com/Zackriya-Solutions/meetily)
- [Meetily Pro 与价格](https://meetily.ai/pro/)
- [anarlog 官方仓库](https://github.com/fastrepl/anarlog)
- [screenpipe 官方仓库](https://github.com/screenpipe/screenpipe)
- [screenpipe 定价](https://screenpi.pe/pricing)
- [Granola 定价](https://www.granola.ai/pricing)
- [Granola 隐私政策](https://docs.granola.ai/help-center/policies/privacy-policy)
- [Krisp 定价](https://krisp.ai/pricing/)
- [Krisp 隐私政策](https://krisp.ai/privacy-policy/)

### 垂直行业

- [voize 官网与价格/部署说明](https://www.voize.ai/us/preise)
- [Raken 移动日报语音输入](https://help.rakenapp.com/en/articles/14478739-how-to-create-a-daily-report-in-the-raken-mobile-app)
- [Raken AI 日报摘要](https://help.rakenapp.com/en/articles/14502801-ai-daily-report-summaries-in-raken)
- [OpenSpace Field Notes](https://www.openspace.ai/products/field/field-notes/)
- [OpenSpace AI Voice Notes](https://www.openspace.ai/blog/openspace-ai-voice-notes/)
- [Abridge 产品页](https://www.abridge.com/product)与[隐私政策](https://www.abridge.com/privacy)
- [Nabla 官网](https://www.nabla.com/)与[安全说明](https://www.nabla.com/security)
- [BigHand Digital Dictation & Speech Recognition](https://www.bighand.com/en-us/our-solutions/digital-dictation-and-speech-recognition/)
