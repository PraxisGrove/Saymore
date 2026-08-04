# 文本润色模型的 OpenAI 兼容适配核查

调研日期：2026-08-04

范围：SenseNova、DeepSeek、阿里云百炼 / Qwen、智谱
GLM、OpenAI、MiniMax、Moonshot Kimi、Google
Gemini、OpenRouter、硅基流动、阶跃星辰，以及火山方舟标准 API 与方舟 Coding Plan
的产品边界。只引用厂商官方文档或官方仓库；“未保证”表示没有在当前官方 API
索引中找到该能力，不等于接口一定不存在。

## 结论先行

第一阶段可以较快完成，但“兼容 OpenAI”只代表可以共享一层 HTTP transport 和 Chat
Completions 编解码核心，不代表十一家可以复制同一个完整配置。

- DeepSeek、Qwen、GLM、OpenAI、MiniMax、Kimi、Gemini、OpenRouter、硅基流动、阶跃星辰
  的普通非流式文本请求，可以共享 `POST {base_url}/chat/completions`、Bearer
  鉴权和 `choices[0].message.content` 解析。
- SenseNova 必须区分两套契约：Token Plan 官方仓库声明其入口兼容 OpenAI；原生
  MaaS / V6.5 的路径、请求内容和响应外形均不是 OpenAI Chat Completions。当前
  Saymore 的 SenseNova preset 使用 Token Plan 做对话，却使用旧 MaaS
  域名获取模型，因此实际上跨了两套 API。
- 模型发现、地域与 Key 绑定、输出长度字段、thinking
  控制、错误结构和内容清洗都必须成为显式
  capability，不能再靠模型名或一个布尔值推断。
- 如果“profile”包含 base
  URL、模型发现、thinking、输出限制和错误处理，则十一家中没有任意两家完全相同。正确结构是共享
  `OpenAiChatCore`，再叠加每家 provider/model capabilities。

## 共同的最小 wire contract

除 SenseNova 原生 MaaS 外，其余 OpenAI-compatible 入口的 POST 最小公共集如下。

必需 header：

```http
Authorization: Bearer <API_KEY>
Content-Type: application/json
```

非流式不需要 `Accept`。OpenRouter 的 `HTTP-Referer`、`X-OpenRouter-Title` 和
`X-OpenRouter-Metadata` 都是可选 header，不应成为连接前提。

最小公共请求体：

```json
{
  "model": "<MODEL_ID>",
  "messages": [
    { "role": "system", "content": "<INSTRUCTIONS>" },
    { "role": "user", "content": "<TEXT>" }
  ]
}
```

`stream` 可省略，或显式设为 `false`。公共核心不应固定加入
`temperature`、输出长度或任何 reasoning
参数：这些字段虽常见，但取值范围、字段名和模型支持度不同。

公共非流式成功响应只应依赖以下最小部分，并容忍其它字段：

```json
{
  "choices": [
    {
      "message": {
        "role": "assistant",
        "content": "<POLISHED_TEXT>"
      },
      "finish_reason": "stop"
    }
  ]
}
```

## 逐家契约

| Provider                   | 精确 base URL                                                                                                                                                                  | Header                                                                                | 最小 POST 与非流式成功响应                                                                                                                                                                                     | `GET /models`                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| -------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| SenseNova Token Plan       | `https://token.sensenova.cn/v1`                                                                                                                                                | 必需 Bearer API Key、JSON                                                             | 官方仓库声明 OpenAI-compatible，示例模型为 `sensenova-6.7-flash-lite`；因此可按 `model + messages` 和 `choices[].message.content` 接入，但官方安装页没有给出完整原始响应 schema                                | **未正式文档化**。不要从“兼容 OpenAI”推断一定存在；当前另用旧 MaaS 模型列表属于跨契约组合。[Token Plan 配置](https://github.com/OpenSenseNova/SenseNova-Skills/blob/main/INSTALL.md)                                                                                                                                                                                                                                                                                 |
| SenseNova 原生 MaaS / V6.5 | `https://api.sensenova.cn/v1/llm`                                                                                                                                              | 必需 Bearer；旧 MaaS 可用 API Key，或由 AK/SK 生成会过期的 API token                  | `POST /chat-completions`；最小为 `model + messages`，V6.5 文档中的文本 `content` 是 content-part 数组；长度字段为 `max_new_tokens`。响应是 `data.choices[0].message` 字符串，不是 `choices[0].message.content` | 旧 MaaS 正式端点为 `GET https://api.sensenova.cn/v1/llm/models`；响应 `{object:"LIST",data:[{id,object:"MODEL",type,owned_by,...}]}`，大小写和字段均非标准 OpenAI。[原生 Chat](https://platform.sensenova.cn/product/APIService/document/356/) [旧模型列表](https://console.sensecore.cn/micro/help/docs/model-as-a-service/nova/overview/Models/GetModelList/) [鉴权](https://console.sensecore.cn/micro/help/docs/model-as-a-service/nova/overview/Authorization/) |
| DeepSeek                   | `https://api.deepseek.com`，官方也兼容末尾 `/v1`                                                                                                                               | 必需 Bearer、JSON                                                                     | `POST /chat/completions`；最小 `model + messages`；标准 `choices[].message.content`，thinking 时另有 `reasoning_content`                                                                                       | 正式支持 `GET /models`：`{object:"list",data:[{id,object:"model",owned_by}]}`。[Chat](https://api-docs.deepseek.com/api/create-chat-completion) [Models](https://api-docs.deepseek.com/api/list-models)                                                                                                                                                                                                                                                              |
| 阿里云百炼 / Qwen          | 北京共享：`https://dashscope.aliyuncs.com/compatible-mode/v1`；生产推荐工作空间：`https://{WorkspaceId}.cn-beijing.maas.aliyuncs.com/compatible-mode/v1`；其它地域使用对应域名 | 必需 Bearer、JSON；Key 与地域、业务空间或套餐绑定                                     | `POST /chat/completions`；最小 `model + messages`；标准 `choices[].message.content`，可有 `reasoning_content`。部分模型仅支持流式                                                                              | Token Plan 材料展示过 `/models`，但没有找到对所有按量付费地域/工作空间入口的统一保证；首版应使用静态 catalog，并把 discovery 做成 capability。[Base URL](https://help.aliyun.com/en/model-studio/base-url) [Chat](https://help.aliyun.com/en/model-studio/qwen-api-via-openai-chat-completions) [Thinking](https://help.aliyun.com/en/model-studio/deep-thinking)                                                                                                    |
| 智谱 GLM                   | `https://open.bigmodel.cn/api/paas/v4/`                                                                                                                                        | 必需 Bearer、JSON                                                                     | `POST chat/completions`；最小 `model + messages`；标准 `choices[].message.content`，可有 `reasoning_content`，响应可附 `request_id`                                                                            | 当前官方 API 索引没有模型列表资源，**不保证**；首版用静态 catalog 或手填模型 ID。[OpenAI 兼容](https://docs.bigmodel.cn/cn/guide/develop/openai/introduction) [Chat](https://docs.bigmodel.cn/api-reference/%E6%A8%A1%E5%9E%8B-api/%E5%AF%B9%E8%AF%9D%E8%A1%A5%E5%85%A8)                                                                                                                                                                                             |
| OpenAI                     | `https://api.openai.com/v1`                                                                                                                                                    | 必需 Bearer、JSON                                                                     | `POST /chat/completions`；最小 `model + messages`；标准 `choices[].message.content`                                                                                                                            | 正式支持 `GET /models`：`{object:"list",data:[{id,object:"model",created,owned_by}]}`。[Chat](https://developers.openai.com/api/reference/resources/chat) [Models](https://developers.openai.com/api/reference/resources/models/methods/list)                                                                                                                                                                                                                        |
| MiniMax                    | 中国 `https://api.minimaxi.com/v1`；国际 `https://api.minimax.io/v1`                                                                                                           | 必需 Bearer、JSON                                                                     | `POST /chat/completions`；当前默认模型为 `MiniMax-M3`，M2.7 系列仍可用；响应有标准核心，也附敏感内容字段和 `base_resp`。思考内容可能出现在 `content` 的 `<think>` 标签                                         | 正式支持 Bearer `GET /models`：`{object:"list",data:[{id,object:"model",created,owned_by:"minimax"}]}`。[Chat](https://platform.minimaxi.com/docs/api-reference/text-chat-openai) [当前 OpenAPI](https://platform.minimaxi.com/docs/api-reference/text/api/openapi-chat-openai.json) [Models](https://platform.minimaxi.com/docs/api-reference/models/openai/list-models)                                                                                            |
| Moonshot Kimi              | 中国：`https://api.moonshot.cn/v1`                                                                                                                                             | 必需 Bearer、JSON；中国与国际平台的 Key/入口不能混用                                  | `POST /chat/completions`；最小 `model + messages`；标准 `choices[].message.content`，thinking 内容单列为 `reasoning_content`                                                                                   | 正式支持 `GET /models`：标准 `object + data[]`，模型项额外含 `context_length`、`supports_image_in`、`supports_video_in`、`supports_reasoning`。[Chat](https://platform.kimi.com/docs/api/chat) [Models](https://platform.kimi.com/docs/api/list-models) [入口排错](https://www.kimi.com/zh-cn/help/kimi-api/api-troubleshooting)                                                                                                                                     |
| Google Gemini              | `https://generativelanguage.googleapis.com/v1beta/openai/`                                                                                                                     | 必需 Bearer Gemini API Key、JSON                                                      | `POST /chat/completions`；最小 `model + messages`；兼容层返回标准 `choices[].message.content`                                                                                                                  | 正式支持 `GET /v1beta/openai/models`；按 OpenAI SDK 的 `models.list()` 使用。兼容层仍是 beta。[OpenAI compatibility](https://ai.google.dev/gemini-api/docs/openai)                                                                                                                                                                                                                                                                                                   |
| OpenRouter                 | `https://openrouter.ai/api/v1`                                                                                                                                                 | 必需 Bearer、JSON；`HTTP-Referer`、`X-OpenRouter-Title`、`X-OpenRouter-Metadata` 可选 | `POST /chat/completions`；可移植最小集为 `model + messages`；标准 `choices[].message.content`，但底层 provider 中断可能产生额外错误结构                                                                        | 正式支持 `GET /models`；文档响应为 `{data:[{id,canonical_slug,name,architecture,supported_parameters,...}]}`，不应要求外层 `object:"list"`。[Quickstart](https://openrouter.ai/docs/quickstart) [Models](https://openrouter.ai/docs/api/api-reference/models/get-models) [Errors](https://openrouter.ai/docs/api/reference/errors-and-debugging)                                                                                                                     |
| 硅基流动 SiliconFlow       | `https://api.siliconflow.cn/v1`                                                                                                                                                | 必需 Bearer、JSON                                                                     | `POST /chat/completions`；使用标准 `model + messages` 与 `choices[].message.content`                                                                                                                           | 正式支持 `GET /models` 与 `data[].id`；Saymore 使用 `type=text&sub_type=chat` 只发现聊天文本模型。[Chat](https://docs.siliconflow.cn/cn/api-reference/chat-completions/chat-completions) [Models](https://docs.siliconflow.cn/en/api-reference/models/get-model-list)                                                                                                                                                                                                |
| 阶跃星辰 StepFun           | `https://api.stepfun.com/v1`                                                                                                                                                   | 必需 Bearer、JSON                                                                     | `POST /chat/completions`；标准按量 API 兼容 OpenAI；HTTP 402 表示余额不足                                                                                                                                      | 正式支持 `GET /models` 与标准 `data[].id`。Step Plan 使用另一入口，不与标准按量配置混用。[OpenAI 兼容](https://platform.stepfun.com/docs/zh/guides/developer/openai) [Chat](https://platform.stepfun.com/docs/zh/api-reference/chat/chat-completion-create) [Models](https://platform.stepfun.com/docs/zh/api-reference/models/list)                                                                                                                                 |

### MiniMax 当前官方文本生成契约

截至 2026-08-04，MiniMax 中国区的 OpenAI-compatible 文本入口是
`https://api.minimaxi.com/v1`，完整非流式端点为
`POST https://api.minimaxi.com/v1/chat/completions`；国际区对应入口是
`https://api.minimax.io/v1`。请求头必须包含 `Authorization: Bearer <API Key>` 和
`Content-Type: application/json`；最小请求体是 `model + messages`。官方
[OpenAI API 兼容指南](https://platform.minimaxi.com/docs/api-reference/text-openai-api)给出中国区
base
URL，[Chat Completions 参考](https://platform.minimaxi.com/docs/api-reference/text-chat-openai)给出请求和响应
schema，[国际区参考](https://platform.minimax.io/docs/api-reference/text-chat-openai)则给出国际
host。旧 `POST /v1/text/chatcompletion_v2` 已被官方标为
deprecated，不应作为新适配目标。

**模型选择。**Saymore 的当前默认模型应为
`MiniMax-M3`。官方[模型发布记录](https://platform.minimaxi.com/docs/release-notes/models)显示
M3 于 2026-06-01 发布，面向 Agent 推理、工具调用、代码、多模态 Chat
和长上下文；[当前按量价格页](https://platform.minimaxi.com/docs/guides/pricing-paygo)把
M3 放在现行语言模型区，并把 M2.5
及更早版本归为历史模型；最新[OpenAI Chat OpenAPI](https://platform.minimaxi.com/docs/api-reference/text/api/openapi-chat-openai.json)也已经使用
M3。`MiniMax-M2.7` 和 `MiniMax-M2.7-highspeed`
仍是现行兼容/低延迟选项；`MiniMax-M2.5`、`MiniMax-M2.5-highspeed`、`MiniMax-M2.1`、`MiniMax-M2.1-highspeed`
和 `MiniMax-M2` 只作为历史兼容项，不应成为新 preset 默认值。部分 HTML Chat/SDK
页面仍显示 M2.7 示例，落后于 OpenAPI、发布记录和价格页；动态目录和 live smoke
test 应作为最终运行时依据。

**模型发现。**MiniMax 正式提供中国区 `GET https://api.minimaxi.com/v1/models`
和国际区对应的 `/v1/models`，同样使用 Bearer API
Key。[模型列表文档](https://platform.minimaxi.com/docs/api-reference/models/openai/list-models)声明其兼容
OpenAI API，并返回 `object: "list"` 与 `data[].id`；当前官方示例包含
`MiniMax-M3`、`MiniMax-M2.7` 和
`MiniMax-M2.5`，另有正式的[单模型查询](https://platform.minimaxi.com/docs/api-reference/models/openai/retrieve-model)。这不是从“OpenAI-compatible”推断出的能力，而是独立文档化的正式端点，可以直接复用
Saymore 当前宽容的 `data[].id` parser。官方没有承诺列表会按当前 Key
的套餐权益过滤，也没有说明 Token Plan Key
是否返回不同目录，因此发现成功不等于当前凭证一定能调用每个模型。

**成功响应和错误语义。**成功响应的主要文本仍在
`choices[].message.content`，但官方响应还包含
`input_sensitive`、`output_sensitive`、对应敏感类型以及
`base_resp.status_code/status_msg`；`base_resp.status_code = 0`
表示成功。严重输入或输出违规时，文档明确说明回复内容为空。因此 adapter 不能把
HTTP 200 或成功解析 JSON 等同于“有可交付润色文本”，必须同时检查
`base_resp`、敏感标记和空
`content`。官方[错误码表](https://platform.minimaxi.com/docs/api-reference/errorcode)列出的关键业务码为：`1000`
未知错误、`1001` 超时、`1002` 频率超限、`1004` 未授权或 Token 不匹配、`1008`
余额不足、`1024` 内部错误、`1026` 输入涉敏、`1027` 输出涉敏、`1033`
下游/系统错误、`1039` Token 限制、`1041` 连接数限制、`1042`
非法或不可见字符比例超限、`2013` 参数错误、`2045` 请求频率增长超限、`2049` 无效
API Key、`2056` 超出 Token Plan 资源限制。该表没有给出业务码与 HTTP
状态的一一映射，当前 Chat OpenAPI 也没有为非 2xx 声明统一 JSON
schema。因此共享层应保留 HTTP status、原始 body 和响应 Header 中供报障使用的
`trace_id`，再由 MiniMax profile 解析可用的 `base_resp`；无法解析时回退为通用
HTTP/body 错误，不能只依赖 HTTP 401/429 或标准 OpenAI `error.message`。

**按量 API 与 Token Plan。**两种资源模式共享 OpenAI-compatible base URL 和
transport，并不需要两套 HTTP
codec；区别是凭证与计费域。[接口概览](https://platform.minimaxi.com/docs/api-reference/api-overview)和
[Token Plan 概要](https://platform.minimaxi.com/docs/token-plan/intro)明确说明
Token Plan Key 与按量计费 API Key 相互独立、不可互换；订阅 Key
消耗套餐额度/积分，按量 Key 按实际 token 从 API 账户余额扣费。Token Plan
的[其他工具接入页](https://platform.minimaxi.com/docs/token-plan/other-tools)确认订阅
Key 也可使用同一个 `https://api.minimaxi.com/v1` OpenAI-compatible 入口。

因此不应虚构一个 MiniMax Coding Plan 专用网关，也不应复制
transport。产品配置层应提供明确的“按量 API / Token
Plan”资源模式，分别保存凭证、默认模型和额度说明；如果 UI
不准备支持两套凭证槽位，第一阶段只上线按量 API。绝不能在 Token Plan
额度耗尽时自动换用按量 Key，或把两种 Key
当作同一凭证回退，因为官方明确把它们定义为不同计费资源。与只允许特定编码工具的套餐不同，MiniMax
[Token Plan FAQ](https://platform.minimaxi.com/docs/token-plan/faq)和工具页允许订阅资源用于日常聊天、翻译、简单写作及
OpenAI-compatible 工具，因此个人用户在 Saymore
中润色文本有官方用途依据；多用户生产后端仍应选择普通按量 API。

### SenseNova 原生响应为什么必须独立

原生 MaaS 的非流式成功响应核心是：

```json
{
  "data": {
    "id": "...",
    "choices": [
      {
        "message": "<POLISHED_TEXT>",
        "role": "assistant",
        "finish_reason": "stop",
        "index": 0
      }
    ],
    "usage": {
      "prompt_tokens": 0,
      "completion_tokens": 0,
      "total_tokens": 0
    }
  }
}
```

因此，Token Plan 可以先按官方的兼容性声明做真实服务 contract test；原生 MaaS
则需要独立请求和响应 codec。不能用原生 MaaS 文档证明 Token Plan 的 `/models`
或响应 schema，也不能反向用 Token Plan 的兼容声明解释原生 MaaS。

## Thinking / reasoning 方言

| Provider   | 控制方式与默认行为                                                                                                                                           | 响应与润色风险                                                                                         |
| ---------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| SenseNova  | Token Plan 安装页没有给出稳定 thinking schema；原生模型卡虽说明可控制 thinking，但当前 Chat 参数文档不足以证明可复用其它厂商字段                             | 作为独立 capability；未验证前不发送 reasoning 字段                                                     |
| DeepSeek   | `thinking.type` 可取 `enabled` 或 `disabled`，当前默认开启；`reasoning_effort` 主要取 `high` 或 `max`，其它档位会映射                                        | thinking 文本在 `reasoning_content`；工具调用多轮需原样回传，但单轮润色只消费最终 `content`            |
| Qwen       | 非标准 `enable_thinking: boolean`，另有 `thinking_budget`；不同模型可能默认开启、始终开启或只支持流式                                                        | thinking 文本通常在 `reasoning_content`；必须按模型声明能力                                            |
| GLM        | `thinking.type` 可取 `enabled` 或 `disabled`；GLM 5.2 默认开启；`reasoning_effort` 支持 `none/minimal/low/medium/high/xhigh/max`，另有 `clear_thinking` 语义 | thinking 文本在 `reasoning_content`；不要因字段同名就与 DeepSeek 共用枚举和默认值                      |
| OpenAI     | 能力取决于具体模型；新 reasoning 模型使用 `reasoning_effort`，并非所有模型都接受相同值                                                                       | 需要 model capability；不能给所有模型固定发送 `none`                                                   |
| MiniMax    | M3 的 `thinking.type` 可取 `adaptive` 或 `disabled`；M2.x 不能关闭。`reasoning_split` 只改变输出外形                                                         | 未分离时 thinking 可能混入 `content` 的 `<think>` 标签；必须分离或安全清洗，并处理空内容或敏感内容输出 |
| Kimi       | K2.6 使用 `thinking.type` 与 `thinking.keep`；2026-08-04 对中国区 `kimi-k3` 实测 `thinking.type = disabled` 同样关闭推理                                     | 最终文本在 `content`，推理在 `reasoning_content`；Saymore 的润色请求默认关闭推理                       |
| Gemini     | `reasoning_effort` 会映射到 Gemini thinking；`none` 只适用于部分 Gemini 2.5 模型，Gemini 2.5 Pro 和 Gemini 3 不能关闭                                        | 省略字段使用模型默认值；无模型能力表时不要固定发送 `none`                                              |
| OpenRouter | 是否支持 thinking 以及字段名由底层模型决定；模型目录提供 `supported_parameters`                                                                              | 由 catalog 驱动参数过滤；OpenRouter 本身不能把所有下游模型统一成一个 reasoning profile                 |

对文本润色，最兼容的第一版策略是默认省略 thinking/reasoning 字段，只消费最终
`content`；只有在模型明确支持且产品确实要求关闭思考时，才由 model capability
写入对应字段。MiniMax M2 等无法关闭 thinking
的模型必须另做内容清洗或不纳入首批默认模型。

## 其它不能忽略的方言

### 输出长度

- DeepSeek、GLM 当前文档使用 `max_tokens`。Qwen 当前同时支持两者，但已把
  `max_tokens` 标为待弃用，并建议新接入使用 `max_completion_tokens`。
- Kimi 已将 `max_tokens` 标为弃用，要求使用 `max_completion_tokens`；MiniMax
  当前 OpenAI Chat 文档也使用 `max_completion_tokens`。
- OpenAI 新模型倾向 `max_completion_tokens`，旧 `max_tokens` 对部分 reasoning
  模型不兼容。
- Gemini 和 OpenRouter 最终受具体模型支持参数约束。

因此，公共最小请求应省略输出长度；Saymore 若必须限制结果，应把字段名和上限放到
model capability，而不是在共享结构中固定 `max_tokens`。

### 模型列表解析

可共享一个宽容的 `data[].id` 解析器，但是否调用 `/models` 必须由 capability
决定：

- 正式支持且接近标准 OpenAI 列表：DeepSeek、OpenAI、MiniMax、Kimi、Gemini。
- 正式支持但 catalog 是路由器扩展结构：OpenRouter。解析时不要要求外层 `object`。
- 未对所有目标入口正式保证：Qwen。
- 当前官方索引未保证：GLM。
- SenseNova Token Plan 未正式文档化；旧 MaaS 有独立 host、路径和非标准大写
  shape。

### 错误和安全输出

共享层可以按 HTTP 状态先分类，但应保留响应 body、request ID 和 `Retry-After` 供
provider adapter 解析。DeepSeek/OpenRouter 的 402、GLM
的字符串业务码、OpenRouter 的上游错误以及 MiniMax
的敏感内容/空回复，都不能统一折叠成“配置错误”。

## 实现分组

“同批实现”不等于“完全相同 profile”。建议按下面顺序推进：

1. **OpenAI、Kimi、Gemini：同一实现批次。**共享 OpenAI chat codec 和标准模型列表
   parser；分别配置 base URL、输出长度和 model-level reasoning capability。
2. **OpenRouter：可与第一批并行，但保留独立 router profile。**transport 和
   `data[].id` parser 可复用，catalog 能力字段、可选 headers
   和上游错误语义不能并入普通厂商 profile。
3. **DeepSeek、MiniMax：共享 chat codec，分别落 profile。**两家都有正式
   `/models`，但 thinking 控制和响应清洗不同；MiniMax 必须先覆盖 `<think>`
   与敏感内容测试。
4. **Qwen、GLM：同一实现批次，使用静态 catalog。**两家复用 chat
   codec，但地域配置、thinking 和错误语义分别实现。
5. **SenseNova：单独收敛契约。**如果产品使用 Token Plan，先用真实 API 验证 chat
   响应和模型发现；如果支持原生 MaaS，则新增独立 codec，不能复用 OpenAI response
   parser。

可复用的不是九个完整 provider profile，而是四类组件：

| 组件                               | 可复用范围                                                                                          |
| ---------------------------------- | --------------------------------------------------------------------------------------------------- |
| `OpenAiChatCore`                   | DeepSeek、Qwen、GLM、OpenAI、MiniMax、Kimi、Gemini、OpenRouter、硅基流动、阶跃星辰                  |
| 宽容的 `data[].id` 模型列表 parser | DeepSeek、OpenAI、MiniMax、Kimi、Gemini、OpenRouter、硅基流动、阶跃星辰；是否启用由 capability 决定 |
| 静态模型 catalog                   | Qwen、GLM，以及任何不希望暴露全量模型的 provider                                                    |
| SenseNova native codec             | 仅 SenseNova 原生 MaaS / V6.5                                                                       |

## 对当前 Saymore 的直接影响

当前实现已经把请求从“按模型名特判”改为显式 `ProviderProfile`：profile 集中声明
base URL、默认模型、模型列表入口、API Key 环境变量和 Chat Completions
方言。共享请求只发送最小公共字段；DeepSeek profile 显式关闭 thinking
并使用其输出长度字段，OpenRouter profile 保留独立错误映射。旧 Provider 配置通过
Provider ID 或 base URL 恢复 profile，不依赖模型名。

桌面 UI 只维护当前 Provider 的一份配置草稿；各 Provider
是否已配置使用一个结构化状态传给卡片列表，不再为每家复制 API Key、模型和
configured 属性。

常规测试使用本地 mock，不需要任何真实 API Key。真实服务 smoke test
默认忽略，可按 Provider 单独执行：

```bash
SAYMORE_LLM_SMOKE_PROVIDER=openai \
OPENAI_API_KEY=... \
cargo test -p template-infra --test chat_completions_llm \
  live_provider_smoke_test_from_environment -- --ignored --nocapture
```

支持的 Provider ID 为
`sensenova`、`deepseek`、`qwen`、`volcengine_ark`、`openai`、`kimi`、`gemini`、`openrouter`、`zhipu_glm`、`minimax`；各自读取
profile 中声明的环境变量。可用 `SAYMORE_LLM_SMOKE_MODEL`
临时覆盖默认模型。远端目录 Provider 会先验证模型列表，静态目录 Provider
会使用推荐目录，然后发送一次最短非流式连接请求；该测试不进入常规 CI。

## 离线验收结果（2026-08-04）

本轮验收只使用本地 Mock、临时配置目录和 Slint 测试目标，不依赖真实 API
Key，也不把测试服务器或测试依赖编入发布版。`httpmock` 仍只存在于
`[dev-dependencies]`。

| Provider       | Chat Completions 契约                                                    | 模型目录策略                                                    | 配置、切换与持久化               | 真实服务       |
| -------------- | ------------------------------------------------------------------------ | --------------------------------------------------------------- | -------------------------------- | -------------- |
| SenseNova      | 通过，共享 `Portable` 最小请求与响应解析                                 | 通过共享远端目录解析器；跨 Token Plan / MaaS 的真实契约仍需 Key | 通过                             | 待 Key         |
| DeepSeek       | 通过，验证关闭 thinking、Bearer、路径与正文解析                          | 通过共享远端目录解析器                                          | 通过                             | 待 Key         |
| Qwen           | 通过，验证最小 Qwen profile 与欠费错误映射                               | 通过静态推荐目录、手填模型保留                                  | 通过                             | 待 Key         |
| 火山方舟       | 通过，验证最小 Ark profile                                               | 通过静态推荐目录、手填模型保留                                  | 通过                             | 待 Key         |
| OpenAI         | 通过，共享 `Portable` 最小请求与响应解析                                 | 通过共享远端目录解析器                                          | 通过                             | 待 Key         |
| Kimi           | 通过，验证关闭 thinking 与独立 profile                                   | 通过共享远端目录解析器                                          | 通过；缓存按 `Kimi` profile 隔离 | 待 Key         |
| Gemini         | 通过，共享 `Portable` 最小请求与响应解析                                 | 通过共享远端目录解析器                                          | 通过                             | 待 Key         |
| OpenRouter     | 通过，验证独立 profile 与 HTTP 402 额度映射                              | 通过共享远端目录解析器                                          | 通过                             | 待 Key         |
| 智谱 GLM       | 通过，验证最小 GLM profile 与模型不存在错误映射                          | 通过静态推荐目录、手填模型保留                                  | 通过                             | 待 Key         |
| MiniMax        | 通过，验证 `reasoning_split`、关闭 thinking、业务错误与空正文拒绝        | 通过共享远端目录解析器及推荐模型保留                            | 通过                             | 待 Key         |
| 硅基流动       | 通过，复用 `Portable` 最小请求、Bearer 与标准响应解析                    | 通过带 text/chat 过滤的远端目录解析器                           | 通过；独立 Provider 缓存作用域   | 待 Key         |
| 阶跃星辰       | 通过，复用 `Portable` 最小请求，并验证 HTTP 402 额度错误                 | 通过共享远端目录解析器                                          | 通过；独立 Provider 缓存作用域   | 待 Key         |
| 自定义兼容接口 | 通过，支持完整 Chat Completions URL、自定义 Header 和空 API Key 本地服务 | 通过 `{base_url}/models` 与手填模型                             | 通过                             | 由用户服务决定 |

共享模型发现契约已验证 `GET` 路径、Bearer Header、`data[].id`
清洗去重、空目录、畸形 JSON、401/403、429 和其它 HTTP 错误。共享 Chat 契约已验证
400、401、404、429、503、无正文成功响应，以及各 Provider profile
的私有错误语义。模型目录会按 Provider、模型列表 URL 和 profile
隔离；失败更新不会覆盖已写盘目录，重载后仍恢复旧模型、选择和刷新时间。

桌面配置入口会为十个内置 Provider 构造并校验候选配置，绑定各自的 canonical base
URL、默认模型和 profile；自定义本地兼容接口可以不填 API
Key。连接测试失败不保存候选配置，成功后才原子保存并启用，已有测试覆盖这一编排契约。

默认离线验收命令：

```bash
cargo test -p template-infra --test model_discovery
cargo test -p template-infra --test chat_completions_llm
cargo test -p template-app settings:: --lib
cargo test -p template-infra json_settings_store::tests --lib
cargo test -p saymore-desktop settings_ui --bin saymore-desktop
cargo test -p saymore-desktop --test models_navigation
```

后续每家最低验收仍应覆盖错误
Key、不可用模型、限流或余额不足、空/过滤输出和真实服务 contract。Streaming
不属于这一阶段；SSE 的 usage 空 `choices`、注释行、`[DONE]`
和流内错误应作为独立工作处理。

## 火山方舟标准 API 与 Coding Plan 必须分成两个 Provider

结论：两者可以共享 OpenAI-compatible
transport，但必须在产品和配置层表现为两个独立
Provider。它们使用不同网关、不同模型命名空间和不同计费权益；混用还可能静默产生额外费用。对于
Saymore 这种普通文本润色桌面应用，第一阶段应只接入**火山方舟标准 API**，不应把
Coding Plan 暴露为通用推理 Provider。

### 契约对比

| 项目              | 火山方舟标准 API                                                            | 方舟 Coding Plan                                                                                                                           |
| ----------------- | --------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------ |
| Provider 建议名称 | `火山方舟（标准 API）`                                                      | `方舟 Coding Plan（仅支持的 AI 编程工具）`                                                                                                 |
| 精确 base URL     | `https://ark.cn-beijing.volces.com/api/v3`                                  | OpenAI-compatible：`https://ark.cn-beijing.volces.com/api/coding/v3`；Anthropic-compatible：`https://ark.cn-beijing.volces.com/api/coding` |
| Chat 端点         | `POST /chat/completions`；标准 API 另正式支持 `POST /responses`             | 供兼容工具使用对应协议；OpenAI-compatible 工具走 `/api/coding/v3` 网关                                                                     |
| 鉴权凭证          | 方舟控制台创建的 API Key，`Authorization: Bearer <ARK_API_KEY>`             | 同一方舟 API Key 体系和获取页面；官方没有定义另一种 secret 类型，但必须通过 Coding Plan 专用网关使用套餐权益                               |
| 计费              | 常规在线推理按实际 token 后付费；不同模型、输入长度和缓存命中使用各自单价   | 月订阅套餐及周期额度；额度耗尽后等待周期恢复，不消耗其它资源包或账户余额                                                                   |
| 模型范围          | 官方完整模型列表中的在线推理模型；请求传 Model ID，范围远大于 Coding Plan   | 只能使用 Coding Plan 当前白名单的 Model Name，或使用 `ark-code-latest` 由控制台选择 / Auto 调度                                            |
| 模型发现          | 官方提供动态维护的文档/控制台模型列表；未找到正式 OpenAI `GET /models` 保证 | 官方提供套餐页、快速开始清单和控制台选择；未找到正式 OpenAI `GET /models` 保证                                                             |
| 正式用途          | 通用模型在线推理和业务应用集成                                              | 个人开发场景，在官方支持的 AI 编程工具中完成项目、学习和工具搭建等编码任务                                                                 |
| 关键误配风险      | 正常按标准 API 计费                                                         | 若把 Key 配到标准 `https://ark.cn-beijing.volces.com/api/v3`，不会消耗 Coding Plan 额度，而会产生额外标准 API 费用                         |

标准 API 的 base URL、Bearer API Key、`POST /chat/completions` 和
`POST /responses`
由[火山方舟产品简介](https://www.volcengine.com/docs/82379/1795150)及[官方 ChatCompletions API](https://api.volcengine.com/api-docs/view?action=ChatCompletions&serviceCode=ark&version=2024-01-01)给出；常规在线推理的按
token
后付费规则见[官方模型价格](https://www.volcengine.com/docs/82379/1544106)。标准入口可用模型应以[官方模型列表](https://www.volcengine.com/docs/82379/1330310)和控制台为准。

Coding Plan 的两个专用 base URL、API Key
获取入口、模型配置方式，以及误用标准网关会额外收费的警告，均来自[官方 Coding Plan 快速开始](https://www.volcengine.com/docs/82379/1928261)。因此，“两个入口”不是
UI 上重复放置同一个服务，而是在保护两个不同的计费域和模型命名空间。

### Coding Plan 当前允许的模型

截至
2026-08-03，[官方 Coding Plan 快速开始](https://www.volcengine.com/docs/82379/1928261)列出的可配置
Model Name 为：

- `doubao-seed-2.1-turbo`
- `doubao-seed-2.0-lite`
- `minimax-m2.7`
- `minimax-m3`
- `glm-5.2`，别名 `glm-latest`
- `deepseek-v4-flash`
- `deepseek-v4-pro`
- `kimi-k2.6`
- `kimi-k2.7-code`

也可以配置 `ark-code-latest`，再由控制台切换具体模型或 Auto 模式。`Auto`
不能直接作为 Model Name。清单和模型上下线会变化，UI 应使用“内置快照 + 允许手填 /
控制台选择”，而不是把这九项永久固化成协议常量。

标准 API 不能复用上面的 Plan 别名清单。其 `model` 参数接受在线推理 Model
ID，完整范围和版本以后缀明确的[标准模型列表](https://www.volcengine.com/docs/82379/1330310)为准。两套模型
ID 即使指向同一模型家族，也不构成可互换的标识符。

### Coding Plan 不能用于 Saymore 的普通文本润色调用

这是明确的官方限制，而不只是谨慎推断。[Coding Plan 套餐概览](https://www.volcengine.com/docs/82379/1925114)最近更新于
2026-07-30，并同时写明：

- 适用场景是个人开发，包括个人项目、学习实践和工具搭建等编码任务；企业级开发应使用火山方舟模型
  API。
- 套餐额度仅在 AI 编程工具中生效。
- Coding Plan 不能用于 API 调用。
- 在非 AI 编程工具中使用其 Base URL 和 API
  Key，可能被识别为滥用或违规，导致订阅停用或账号封禁。
- 同一订阅套餐额度可在官方支持的工具中共享；耗尽后等待下个周期恢复，不会转而消耗其它资源包或账户余额。

该页虽然把 OpenClaw、Hermes Agent 等 Agent
工具列入支持工具，但这不等于授权任意普通应用使用套餐。它们是官方明确列出的受支持
AI 编程/Agent 工具；Saymore
的文本润色功能既不是该列表中的工具，也不是编码任务。因此，不应依据“协议能调通”推导“套餐允许使用”。

### 用量与模型限制

Coding Plan
同时受三层额度窗口约束：[套餐概览](https://www.volcengine.com/docs/82379/1925114)说明每
5 小时额度按首次请求形成的周期刷新，周额度每周一 `00:00:00`
重置，月额度在每个订阅月首日 `00:00:00`
重置。精确额度、TPM、模型抵扣系数和活动折扣会变化，应在控制台展示，不应写进
Provider 协议常量。

官方还要求必须使用 Coding Plan 支持的模型和专用 base
URL：其它模型无法使用套餐；错误网关不会消耗 Plan 额度，并可能产生额外 API
费用。这意味着 provider adapter 不能在 Coding Plan 与标准 API 之间自动
fallback，也不能把一边的模型列表用于另一边。

### 对 Saymore 的实现决策

1. 第一阶段新增 `volcengine-ark`，使用标准
   `https://ark.cn-beijing.volces.com/api/v3`，复用
   `OpenAiChatCore`，模型策略为受控 preset + 手填 Model ID。
2. 不为文本润色功能上线 `volcengine-coding-plan`。若未来产品增加官方允许的
   Coding/Agent 工具形态，再作为独立 Provider
   评估，而不是标准方舟的“套餐计费”开关。
3. 即使未来两者都存在，也应分别保存 Provider ID、base
   URL、模型目录、计费说明、额度错误映射和凭证槽位。凭证类型虽相同，也不应自动跨
   Provider 复用或自动回退，以免产生意外费用。
4. 两者都不能假定支持
   `GET /models`。首版使用官方清单快照和自定义模型输入，并在文档中标注快照日期。

## 下一批中国模型 Provider 的适配优先级

本节补充调研日期为 2026-08-03。代码现状与 UI 截图略有差异：当前仓库已经内置
`SenseNova`、`DeepSeek`、`OpenAI`、`Kimi`、`Gemini`、`OpenRouter` 六个 Provider
和一个自定义兼容入口；`ChatCompletionsProfile` 只有
`Portable`、`DeepSeek`、`OpenRouter` 三种方言。模型发现器固定发送 Bearer
`GET`，并只解析 OpenAI 形状的 `data[].id`。因此，判断“能否如法炮制”要同时检查
Chat Completions、鉴权、模型发现和响应语义，不能只看 SDK 是否接受 OpenAI
`base_url`。

### 官方契约对比

| 候选 Provider        | 稳定按量 base URL                                                                          | Bearer + `POST /chat/completions` | `GET /models`                                                               | 对当前实现的复用结论                                                                              |
| -------------------- | ------------------------------------------------------------------------------------------ | --------------------------------- | --------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| 百度千帆 V2          | `https://qianfan.baidubce.com/v2`                                                          | 是                                | 是，`GET /v2/models`，返回 `data[].id`；可按 `type` 区分模型用途            | 可直接复用 `Portable` 和当前 parser；只需新增 Provider descriptor，并在 UI 过滤非 chat 模型       |
| SiliconFlow          | `https://api.siliconflow.cn/v1`                                                            | 是                                | 是，`GET /v1/models`，返回 `data[].id`；支持 `type=text&sub_type=chat`      | 可直接复用 `Portable`；模型请求最好带服务端过滤条件，或在客户端按能力过滤                         |
| 阶跃星辰 StepFun     | `https://api.stepfun.com/v1`                                                               | 是                                | 是，`GET /v1/models`，标准 `data[].id`                                      | 可直接复用 `Portable` 和当前 parser；普通 API 与 `step_plan` 必须分开                             |
| Xiaomi MiMo 按量 API | `https://api.xiaomimimo.com/v1`                                                            | 是；也接受 `api-key` header       | 是，`GET /v1/models`，标准 `data[].id`                                      | 最小文本调用可复用 `Portable`；目录同时包含文本、ASR、TTS，必须过滤，Token Plan 使用另一 base URL |
| 阿里云百炼 Qwen      | 北京公共入口为 `https://dashscope.aliyuncs.com/compatible-mode/v1`；工作空间和地域入口不同 | 是                                | 官方只对部分套餐/入口给出兼容列表能力，不能当作所有地域和工作空间的统一保证 | 共享 HTTP transport，但首版需要地域/工作空间配置或限定北京公共入口，并使用受控目录 + 手填模型     |
| MiniMax              | `https://api.minimaxi.com/v1`                                                              | 是                                | 是，`GET /v1/models`，标准 `data[].id`                                      | 共享 transport 和 parser；思考模式、`reasoning_split`、输出上限字段需要独立 profile               |
| 智谱 GLM             | `https://open.bigmodel.cn/api/paas/v4`                                                     | 是                                | 当前官方 API 索引未给出稳定的 OpenAI `GET /models` 契约                     | 共享 transport；使用静态目录 + 手填模型，思考参数和错误语义使用独立 profile                       |
| 腾讯混元直接入口     | `https://api.hunyuan.cloud.tencent.com/v1`                                                 | 是                                | 当前官方兼容文档未给出 `GET /v1/models` 契约                                | 最小调用可复用 `Portable`；先用受控目录，且应另行评估腾讯 TokenHub，而不要混成同一个 Provider     |

百度千帆的
[V2 快速开始](https://cloud.baidu.com/doc/qianfan/s/rmh4stn9m)明确给出 OpenAI
SDK、Bearer 鉴权和
`POST /v2/chat/completions`；其[模型列表 API](https://cloud.baidu.com/doc/qianfan-api/s/Dmba8k71y)给出标准
`GET /v2/models` 与 `data[].id`，记录还包含 `type`
等能力字段。普通文本润色不需要千帆私有参数；未来若开放思考模式，再依据[文本生成 API](https://cloud.baidu.com/doc/qianfan-api/s/3m7of64lb)处理
`thinking.type`。

SiliconFlow
的[对话 API](https://docs.siliconflow.cn/en/api-reference/chat-completions/chat-completions)使用
Bearer
`POST /v1/chat/completions`，其[模型列表 API](https://docs.siliconflow.cn/en/api-reference/models/get-model-list)返回标准列表并提供
`type`、`sub_type`
过滤。它是聚合平台，用户价值在于用一个国内可访问入口覆盖多个开源和国产模型；这与
OpenRouter 的产品定位有重叠，但网络可达性、计费和可用模型并不相同，应保留独立
Provider ID。

StepFun 的
[OpenAI 迁移指南](https://platform.stepfun.com/docs/zh/guides/developer/openai)、[Chat Completions API](https://platform.stepfun.com/docs/zh/api-reference/chat/chat-completion-create)和[模型列表 API](https://platform.stepfun.com/docs/zh/api-reference/models/list)共同确认标准
Bearer、`https://api.stepfun.com/v1` 和
`data[].id`。`https://api.stepfun.com/step_plan/v1` 是另一套餐入口；和火山
Coding Plan 一样，不能把套餐网关当成普通按量 Provider 的 base URL 别名。

MiMo
的[首次调用文档](https://mimo.mi.com/docs/zh-CN/quick-start/summary/first-api-call)区分按量入口
`https://api.xiaomimimo.com/v1` 与 Token Plan
入口，且两者凭证不能混用；[OpenAI Chat API](https://mimo.mi.com/docs/zh-CN/api/chat/openai-api)支持
Bearer；[模型列表 API](https://mimo.mi.com/docs/zh-CN/api/model/list-models)返回标准列表。因为列表中同时存在
`mimo-v2.5`、ASR 和 TTS 模型，当前只取 `data[].id`
的实现会把不可用于文本润色的模型也展示出来，适配时必须补能力过滤或静态
allowlist。

Qwen
的[兼容模式文档](https://help.aliyun.com/en/model-studio/qwen-api-via-openai-chat-completions)可以复用最小
Chat Completions
请求，但[地域与工作空间 endpoint 文档](https://help.aliyun.com/en/model-studio/base-url)表明它不是一个全球统一
base URL。MiniMax
的[OpenAI 兼容 API](https://platform.minimaxi.com/docs/api-reference/text-chat-openai)、[思考模式说明](https://platform.minimaxi.com/docs/api-reference/text-openai-api)和[模型列表 API](https://platform.minimaxi.com/docs/api-reference/models/openai/list-models)确认了共享传输层，但
`thinking.type`、`reasoning_split` 和 `max_completion_tokens` 不应塞进通用
profile。GLM
的[OpenAI SDK 迁移文档](https://docs.bigmodel.cn/cn/guide/develop/openai/introduction)和[对话补全文档](https://docs.bigmodel.cn/api-reference/%E6%A8%A1%E5%9E%8B-api/%E5%AF%B9%E8%AF%9D%E8%A1%A5%E5%85%A8)同样支持最小公共请求，而[思考模式文档](https://docs.bigmodel.cn/cn/guide/capabilities/thinking)定义了
`thinking.type`、`reasoning_effort` 等私有语义，因此需要独立 profile。

腾讯混元的[OpenAI 兼容接口文档](https://cloud.tencent.com/document/product/1729/111007)给出
Bearer、固定 base URL 和标准响应，但没有给出直接入口的模型发现契约。腾讯另有
[TokenHub OpenAI 兼容入口](https://cloud.tencent.com/document/product/1823/130078)，它有自己的网关和模型列表能力；这是另一项聚合服务评估，不应让
`hunyuan` descriptor 静默切换到 TokenHub。

### 推荐顺序与批次

综合现有 profile 的复用成本、国内用户价值和官方文档确定性，建议顺序如下：

1. **百度千帆 V2**：品牌和国内用户价值高，按量入口、Bearer、Chat
   Completions、模型发现四项均有明确官方契约；是最适合验证“只新增
   descriptor”路径的首个 Provider。
2. **SiliconFlow**：接入成本同样最低，一次适配可覆盖较广的国产模型；需要做文本/chat
   模型过滤，避免把图像、音频模型放进润色下拉框。
3. **Qwen /
   阿里云百炼**：用户价值很高，优先级应高于纯粹按接入难度排序；但它不是简单复制，必须先决定首版仅支持北京公共入口，还是让用户选择地域/工作空间。
4. **StepFun 标准
   API**：标准契约完整、改动小；按量入口可直接进入第一工程批次，`step_plan`
   不在本阶段范围内。
5. **Xiaomi MiMo 按量
   API**：当前标准接口和模型列表都很完整，接入成本低；模型较新且目录混合多模态类型，首版应只展示明确的文本模型。
6. **MiniMax**：用户价值高且有标准模型发现，但思考输出和 token 参数值得建立独立
   `MiniMax` profile 后再上线，避免后续清洗逻辑散落。
7. **智谱 GLM**：Chat 核心兼容，但缺少可靠动态模型发现，且思考方言需要独立
   profile；以静态目录 + 手填模型上线更稳妥。
8. **腾讯混元直接入口**：最小请求不难，但直接入口缺少动态模型发现，腾讯又存在独立
   TokenHub 产品；先确认产品选择和模型目录，再实施可减少重复 Provider。

第一工程批次建议同时做 **千帆、SiliconFlow、StepFun**：三者共享当前 `Portable`
Chat transport 和 `data[].id` parser，改动主要是 Provider
descriptor、卡片、默认模型、环境变量与 smoke
配置。若第一批只能选两个，则选千帆和 SiliconFlow。Qwen
应作为紧随其后的高价值批次，而不是为了追求数量，把地域和模型发现问题隐藏在一个写死的北京
base URL 后面。

### Profile 与目录边界

- **只新增 Provider descriptor，继续使用
  `Portable`**：千帆、SiliconFlow、StepFun；MiMo
  的最小非思考文本调用也属于这一类。
- **共享 transport，但新增独立 ChatCompletions
  profile**：Qwen、MiniMax、GLM。独立 profile 负责私有思考字段、输出 token
  字段、响应清洗和错误语义，不复制 HTTP client。
- **暂用受控目录 + 手填模型**：Qwen、GLM、腾讯混元、火山方舟标准
  API。没有官方稳定 `GET /models` 契约时，不应伪造动态发现。
- **动态目录仍需能力过滤**：千帆、SiliconFlow、MiMo。当前 parser 只保留
  ID，无法判断 text/chat/ASR/TTS；SiliconFlow 已通过官方
  `type=text&sub_type=chat` 查询参数在服务端过滤，其余 Provider
  应扩展带能力元数据的领域类型，或为首版增加 allowlist。
- **套餐/聚合网关必须是不同 Provider**：MiMo Token Plan、Step Plan、腾讯
  TokenHub、火山 Coding Plan 均不能作为按量入口的隐藏 base URL
  切换。凭证、模型命名空间、额度和使用条款需要独立保存和提示。

所以，“各家都兼容 OpenAI”足以让**传输层**快速复用，却不足以保证整个 Provider
只改常量。真正应抽象并复用的是最小 Chat Completions transport；需要按 Provider
显式声明的是 base URL 范围、模型目录策略、思考/输出方言、计费入口和错误语义。
