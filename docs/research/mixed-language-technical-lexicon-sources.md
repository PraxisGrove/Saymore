# 中英混合技术词库的公开数据源与 Saymore MVP 方案

调研日期：2026-07-15
范围：面向中文用户在听写中夹杂英文软件、AI、开发者工具、产品和品牌名时，可复用的数据源、许可边界与产品方案。本文只引用数据维护方、标准组织和许可证发布方的一手资料。许可证结论是工程选型建议，不代替正式法律意见。

## 结论

1. **不存在一个可以直接打包、同时覆盖技术专名、标准大小写、中文叫法和 ASR 误识别形式的万能公开词库。**公开目录最擅长提供 `GitHub`、`TypeScript`、`Kubernetes` 这类标准写法；它们通常不提供中文用户实际说出的形式，也不知道 ASR 会把它识别成什么。
2. **Saymore MVP 应采用混合方案：产品维护的公开基础词库 + 用户本地词典 + 检索后再交给 LLM。**公开数据只作为构建期候选来源，不把完整上游数据库原样塞进客户端或提示词。
3. **基础词库应由 Saymore 审核后激活。**首批优先从 GitHub Linguist、Homebrew Cask、LF AI & Data Landscape 和 CNCF Landscape 取候选；WinGet、Wikidata、NVD 等只做补漏候选。
4. **中英混合不能继续按整句话的 `zh-Hans` 或 `en` 二选一过滤词条。**中文句子应同时查询用户词典、英文技术词条、拆词/连词变体和少量已确认的中文音译别名。
5. **LLM 能提升冷启动纠错，但不能替代真实反馈。**它可以根据当前句子和候选词判断 `open ai -> OpenAI`，却无法证明这是用户长期偏好，也不能安全地把一次猜测自动写入正式词典。

## 公开数据能解决什么

| 问题 | 公开数据是否能解决 | 说明 |
| --- | --- | --- |
| 标准大小写和标点 | 较好 | 例如 `TypeScript`、`C++`、`Docker Desktop` |
| 拆词/连词变体 | 可派生 | 可从标准词生成 `open ai`、`open-ai`、`openai` 等受控匹配形式 |
| 产品类别和领域 | 部分 | Landscape 数据可提供 AI、云原生、数据库等类别 |
| 中英文名称对应 | 部分 | Wikidata 或 Microsoft 术语可提供部分对应关系，但完整性和许可需要单独处理 |
| 中文用户如何读英文名 | 基本不能 | 数据源通常没有中国用户口语读法 |
| 某个 ASR 会识别成什么错词 | 不能 | 必须来自固定录音测试、用户确认或人工维护 |
| 用户长期偏好 | 不能 | 必须来自本地用户词典或可靠的用户反馈 |

因此，公开词库是**标准写法候选源**，不是自动学习证据，也不是 ASR 声学热词的完整替代品。

## 数据源评估

下列规模是调研日从官方原始文件或 JSON 端点得到的快照，只用于比较量级，不应作为永久产品承诺。

| 数据源 | 覆盖与质量 | 规模和更新 | 许可证与明确结论 | Saymore 用法 |
| --- | --- | --- | --- | --- |
| [GitHub Linguist](https://github.com/github-linguist/linguist) [`languages.yml`](https://github.com/github-linguist/linguist/blob/main/lib/linguist/languages.yml) | GitHub 使用的编程、标记、数据和文本语言名称；标准大小写可靠，含显式 alias | 815 个语言名、424 个显式 alias，原始 YAML 约 162 KB；有版本发布和持续维护 | [MIT](https://github.com/github-linguist/linguist/blob/main/LICENSE)：允许商用、修改、再分发；分发时保留版权与许可文本 | **首批采用。**只取语言名和 alias，不取 grammar 等另有许可证的 vendored 内容 |
| [Homebrew Cask](https://github.com/Homebrew/homebrew-cask) 与[官方 JSON API](https://formulae.brew.sh/docs/api/) | macOS GUI、CLI 和插件产品名；官方 Cask 规范要求 `name` 保留软件的正确大小写、空格和标点 | 7,704 个当前 cask；完整 API 约 16 MB；仓库高频更新 | [BSD-2-Clause](https://github.com/Homebrew/homebrew-cask/blob/main/LICENSE)：允许商用、修改、再分发；源或二进制分发需保留版权、条件和免责声明 | **首批采用。**只抽取 `token`、`name`、必要的 homepage；不打包图标和安装信息 |
| [LF AI & Data Landscape](https://github.com/lfai/lfai-landscape) | AI、ML、数据领域的项目和产品；通常要求 GitHub 托管且达到 300 stars，领域相关性高 | 471 个 item，`landscape.yml` 约 167 KB；成员数据夜间更新，其他条目持续 PR 维护 | README 明确 `landscape.yml` 可按 [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/) 使用：可商用、修改、再分发，但要署名、链接许可证并标明修改。Crunchbase 字段不在 Apache 授权内且仅限 Landscape 项目，logo 归品牌方 | **首批采用。**只取 `name`、category 和项目 URL；明确排除 `crunchbase`、logo 和来源不明描述 |
| [CNCF Landscape](https://github.com/cncf/landscape) | 云原生、容器、数据库、可观测性、开发平台等项目和产品；通常要求 300 stars 或明确产品资格 | 2,426 个 item，`landscape.yml` 约 1.13 MB；官方说明每日生成 | 与 LF AI 相同，`landscape.yml` 可按 [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/) 使用；Crunchbase 数据和 logo 必须排除 | **首批采用但严格筛选。**适合补充 `Kubernetes`、`Prometheus` 等，不能把全部 2,000 多项默认激活 |
| [SPDX License List](https://spdx.org/licenses/) 与[机器可读数据](https://github.com/spdx/license-list-data) | 标准许可证全名和标识符，适合 `MIT`、`Apache-2.0`、`AGPL` 等开发者口述 | 当前 729 个 license 记录，汇总 JSON 约 334 KB；源仓库说明大致按季度发布并打版本 tag | SPDX 2.3 规定 SPDX Metadata 使用 [CC0-1.0](https://spdx.github.io/spdx-spec/v2.3/document-creation-information/#62-data-license-field)，但生成数据仓库本身要求另查源仓库许可。**MVP 只采用少量标准 identifier/full name，不批量复制许可证全文；批量再分发前再做一次许可确认。** | 小范围补充，不是主要品牌词源 |
| [WinGet Community Repository](https://github.com/microsoft/winget-pkgs) 与[Manifest 文档](https://learn.microsoft.com/en-us/windows/package-manager/package/manifest) | Windows 软件的 `PackageName`、`Publisher`、`PackageIdentifier` 和 `Moniker`；有自动校验及可能的人工复核，但由发布者和社区共同提交，命名一致性弱于 Homebrew | 官方称有数千应用的 manifest；按版本保存导致大量重复，仓库更新频繁 | [MIT](https://github.com/microsoft/winget-pkgs/blob/master/LICENSE)：允许商用、修改、再分发，保留版权和许可 | **二期候选源。**按唯一包去版本、交叉验证标准展示名后再进入审核队列，不原样打包仓库 |
| [Wikidata](https://www.wikidata.org/wiki/Wikidata:Data_access) 与[数据库下载](https://www.wikidata.org/wiki/Wikidata:Database_download/en) | 通用知识图谱，可按 software、programming language、AI model 等类别筛选英文 label、中文 label 和 alias；覆盖最广，也最容易出现歧义、旧称和社区噪声 | 120M+ items；完整 JSON dump 每周生成，2026 年快照的 bzip2 文件约 100 GB，另有每日增量 | 主、Property、Lexeme 和 EntitySchema 命名空间的结构化数据为 [CC0](https://creativecommons.org/publicdomain/zero/1.0/)；可商用、修改和再分发，无强制署名 | **只做构建期召回。**用类型、语言、sitelink 和其他可信源交叉过滤；绝不把全量 dump 带进客户端 |
| [NVD Official CPE Dictionary](https://nvd.nist.gov/products/cpe) 与[产品 API](https://nvd.nist.gov/developers/products) | 安全领域的软件和系统规范名；覆盖商业、开源和遗留产品，但按版本产生大量重复，CPE component 常为小写和下划线形式 | 约 177 万 CPE names、42 万以上 match strings；官方字典夜间更新 | NIST 明确 CPE Dictionary 在美国不受版权约束、欢迎署名。若使用 API，[API 条款](https://nvd.nist.gov/developers/terms-of-use)要求避免暗示背书，建议展示指定通知；修改 API 内容后不得继续把修改内容归为 NVD 原文 | **仅低置信候选。**优先使用字典发布物而非客户端实时 API；不能直接把 CPE component 当展示名 |
| [GitHub Advisory Database](https://github.com/github/advisory-database) | 多个开发包生态中出现过安全公告的包名；GitHub-reviewed 数据有人工策展，但覆盖只限有公告的包，registry 名通常不是产品展示名 | 数万条 advisory，持续更新；按 ecosystem 和 advisory 保存，重复很多 | [CC BY 4.0](https://github.com/github/advisory-database/blob/main/LICENSE.md)：允许商用、修改、再分发，需要署名、许可链接和修改说明 | 只用于包名补漏或交叉验证，不作为客户端基础词库 |
| Microsoft Terminology Collection | Microsoft 产品和通用 IT 中英术语；对中文术语很有吸引力 | [微软 Local Language Program FAQ](https://download.microsoft.com/download/C/6/4/C64EB1BF-A375-4F81-A070-A1B683BAD4AC/LLP%20FAQ%20Final.pdf)称英文约 30,000 词、至少 2,500 个基础 IT 词、约每三个月更新；[当前 Learn 文档](https://learn.microsoft.com/en-us/contribute/content/style-quick-start)仍指向术语下载 | FAQ 允许在适当署名下导入自有术语库，但引用的专用 License Agreement 链接现已失效；这不是 MIT/CC 等通用开放许可证。**在拿到并审查当前下载包内许可证前，不得随 Saymore 商业客户端打包。** | 暂不进入 MVP；许可确认后，可作为中英术语候选源单独接入 |

### 明确不建议直接打包的来源

- **npm registry 全量数据。**npm 的[官方许可页](https://docs.npmjs.com/policies/npm-license/)明确说，发布到 registry 的数据属于各发布者，各 package 独立授权，不受 npm 自身许可证覆盖。包名还存在海量低质量、一次性和恶意条目，不能形成统一可再分发词库。
- **OSV 聚合 dump。**OSV 的[官方来源清单](https://google.github.io/osv.dev/data/)显示上游分别使用 CC0、CC BY、CC BY-SA、MIT、Apache 等不同许可。OSV 服务代码的 Apache-2.0 不能覆盖聚合数据；逐条保存上游许可的成本不适合 MVP。
- **Stack Overflow tags。**官方[许可说明](https://stackoverflow.com/help/licensing)按发布时间分别采用 CC BY-SA 2.5、3.0 和 4.0。商用并非禁止，但署名与 ShareAlike 义务会使 Saymore 的混合词库再分发复杂；tag 又常被强制小写和连字符化，不能作为标准展示写法。

## 三种方案比较

| 方案 | 优点 | 主要问题 | 结论 |
| --- | --- | --- | --- |
| 直接打包公开词库 | 上线快、覆盖广、无需从零收集 | 噪声、同名冲突、许可证与署名混杂、体积大、不能提供 ASR 错误形式；更新后可能静默改变输出 | **不采用全量直包。**公开源只进构建期候选 |
| Saymore 维护公开基础词库 | 可控制质量、大小写、别名、领域和歧义；可加入中文口语/ASR 专用映射；便于测试和回滚 | 需要持续审核、版本和许可证治理 | **作为 MVP 主体。**公开维护有利于用户提交和审计，但激活权由产品评审控制 |
| 仅用户本地词典 + LLM | 隐私好、个性化强、没有第三方词库分发负担 | 冷启动差；用户操作重；LLM 有幻觉、成本和不确定性；不能提升 ASR 前端热词命中 | **只能作为补充。**不能单独承担中文用户的常见技术词冷启动 |

推荐组合如下：

```text
Saymore 审核的基础词库（冷启动）
                +
用户本地词典 / 已确认别名（最高优先级）
                +
当前句子的相关词检索（缩小候选）
                ↓
ASR 热词（Provider 支持时） + LLM 受约束纠错
                ↓
确定性标准写法规范化
```

## Saymore MVP 的词条设计

基础词库和本地词典应使用同一领域结构，但来源和优先级不同：

```text
TermEntry
  id
  canonical             # OpenAI
  lexical_aliases[]     # open ai, open-ai
  spoken_zh_aliases[]   # 只保存人工或测试确认的中文口语/误识别形式
  category              # ai/product, language, database, cloud...
  locales[]             # zh-Hans, en；不是整句语言过滤器
  ambiguity             # safe / needs_context，例如 Go、Rust、Swift
  state                 # candidate / active / rejected / deprecated
  source
  source_id
  source_revision
  source_license
  confidence
  updated_at
```

必须区分两类 alias：

- `lexical_aliases` 可以从标准写法保守派生，例如大小写、空格、连字符和字母缩写分隔。
- `spoken_zh_aliases` 不能靠通用拼音算法批量生成。英文发音、中文习惯读法和具体 ASR 错误没有稳定的一一映射，只能来自固定测试集、人工审核或用户明确确认。

## 中英混合检索和 LLM 使用

当前“先把整句推断为 `zh-Hans` 或 `en`，再只读同语言词条”的做法会漏掉最重要的场景。MVP 应改为：

1. 对 ASR 原文做 NFKC、大小写折叠和分隔符规范化，同时保留原文位置。
2. 对中文句子也始终查询英文技术词条；用字符脚本识别只是召回特征，不是排除条件。
3. 依次召回：用户本地词典、最近确认词、用户选择的领域包、基础词库。
4. 同时匹配连写/拆写和缩写形式，例如 `open ai`、`git hub`、`A P I`；只对已知词生成这些变体。
5. 对 `Go`、`Rust`、`Swift`、`Apple` 等普通词/品牌同形项要求上下文命中，不做无条件替换。
6. 只把排名靠前且与当前文本有证据关系的 20--50 个候选传给 LLM，用户词优先于基础词。
7. 提示 LLM 只能在语音文本已有直接或近似证据时采用候选，不得为了使用词表而新增品牌名。
8. LLM 后执行确定性标准写法规范化；如果 LLM 新引入的专名不在允许候选中，则保留审计记录或拒绝该局部替换。

示例提示上下文不应是完整词典，而应是：

```json
[
  {"id":"product.openai","heard":"open ai","canonical":"OpenAI"},
  {"id":"product.github","heard":"git hub","canonical":"GitHub"},
  {"id":"language.rust","canonical":"Rust","requires_context":true}
]
```

如果 ASR Provider 支持热词，应在识别前发送同一批高优先级词；LLM 只负责识别后的语言判断和整理。只靠 LLM 后处理无法找回被 ASR 完全遗漏的专名，也无法降低首轮声学误识别。

## 构建、更新和许可证治理

MVP 不需要客户端联网同步上游词库。推荐构建期流程：

```text
固定上游 tag/commit
  -> 字段白名单抽取
  -> 许可证和来源元数据写入
  -> 去重、歧义检测、跨源交叉验证
  -> 人工审核 candidate diff
  -> 生成小型只读 runtime index
  -> 随 Saymore 版本发布
```

实施要求：

- 首版只激活约 300--800 个高频词，而不是追求数万条覆盖；其余留在构建期候选库。
- 原创词条和不同上游的派生文件分开保存，生成时再合并，避免把许可证边界抹掉。
- 发行包包含第三方 notices：MIT/BSD 保留版权和许可，CC BY 保留来源、许可证链接和修改说明，Apache 来源检查 `NOTICE`。
- 对 Landscape 抽取器建立硬性字段白名单，并测试输出中没有 `crunchbase`、logo、描述和图片 URL。
- 每个刷新版本记录 source revision 和哈希；上游 diff 必须经人工审核，不能自动把新增项目全部激活。
- 产品名和项目名可能是商标。开放许可证通常不授予商标权；词库页面和发行说明应写明“品牌归各权利人所有，收录仅用于拼写纠正，不代表合作或背书”，且不打包第三方 logo。
- 基础词库可以公开接受 PR，但贡献协议必须说明贡献者有权提交名称、别名和发音信息，并同意 Saymore 对该数据的再分发许可。

## MVP 决策

Saymore 不应“下载一个大词库然后直接使用”，也不应把所有压力交给用户本地词典和 LLM。建议确定为：

> 建立 Saymore 自己维护、可公开审计的小型基础词库；使用许可清晰的一手数据源生成候选，人工审核后激活；用户本地词典拥有最高优先级；每次只检索当前中文夹英文句子相关的词条交给 ASR/LLM，最终再做确定性规范化。

第一版来源优先级：

```text
GitHub Linguist
  -> Homebrew Cask
  -> LF AI & Data Landscape
  -> CNCF Landscape
  -> 少量人工维护的主流 AI/产品名
  -> WinGet / Wikidata / NVD 只做补漏候选
```

这个方案能解决常见技术专名的冷启动，又不会把公开数据的噪声、体积和许可证复杂度带到每个用户客户端。真正的中文读法和 ASR 错误映射，则随着本地测试集与用户确认机制逐步补充。
