# ASR Provider 声明语言能力

Saymore 不维护一份跨模型的全局语言承诺，而要求每个 ASR Provider 声明其支持的语言、自动检测、句内混说、地区变体、热词、流式和离线能力。产品只展示当前 Provider 声明支持的语言策略，避免把某个底层模型的理论能力错误包装成所有 Provider 都能兑现的产品能力。

## Consequences

语言选项和验收矩阵必须按 Provider 生成；`AutoDetect`、`Preferred` 和 `Fixed` 策略只有在当前 Provider 能力允许时才可选择。默认采用 `Preferred(系统语言)`，仍允许识别其他语言；Provider 不支持偏好语言时降级为其支持的 `AutoDetect` 或 `Fixed`。新增 Provider 必须通过能力契约测试，LLM 精炼不能补充或伪装 ASR 不具备的语言能力。
