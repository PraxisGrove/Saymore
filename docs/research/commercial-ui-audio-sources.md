# 可商用的开源录音提示音调研

> 调研日期：2026-07-14
>
> 范围：语音识别开始、结束所需的短 UI 提示音
>
> 方法：只采用 Creative Commons 官方条款、素材作者官方页面和素材平台官方页面。本文不是法律意见。

## 结论先行

Saymore 应优先采用 **CC0** 音频。CC0 允许复制、修改、分发和商用，不要求署名，也允许把原始音频随开源仓库和安装包重新分发；这比“免费商用”素材站的自定义许可更适合开源桌面应用。[CC0 1.0 官方说明](https://creativecommons.org/publicdomain/zero/1.0/deed.en)

首轮试听建议使用两组候选：

1. [Kenney Interface Sounds](https://kenney.nl/assets/interface-sounds)：100 个 CC0 OGG，优先试听 `open_001` / `close_001`、`maximize_007` / `minimize_007`、`switch_001` 至 `switch_007`。开始和结束应使用同一音色家族，靠上行/下行或明暗变化区分，不要混用两套音色。
2. Freesound 的 [beep_up.wav](https://freesound.org/people/paep3nguin/sounds/388046/) / [beep_down.wav](https://freesound.org/people/paep3nguin/sounds/388047/)：作者分别说明其用于表示开启和关闭，两者约 0.252 秒，当前素材页均标为 CC0。这是语义最贴近录音开始/结束的一对现成候选。

若这两组仍显得太“游戏化”，可从 [Kenney UI Audio](https://kenney.nl/assets/ui-audio) 的 50 个 CC0 音效中试听 `switch1` 至 `switch38`，或在 Freesound 仅按 CC0、短于 1 秒继续筛选。不要仅凭搜索结果或文件名入库，必须打开每个素材页再次确认许可。

## 许可边界

| 许可 | 可商用 | 署名 | 随源码/安装包再分发 | Saymore 建议 |
| --- | --- | --- | --- | --- |
| CC0 1.0 | 是 | 不要求 | 是，可复制、修改和分发 | **首选**；仍保存作者、原始链接、下载日期和许可快照作为来源记录 |
| CC BY 4.0 | 是 | 必须 | 是，但必须保留署名、许可链接并标明修改；不得施加限制他人行使许可权利的法律条款或有效技术措施 | 备选；在 `THIRD_PARTY_NOTICES` 和应用内许可页完整署名，涉及 DRM 分发平台时先评估 |
| CC BY-NC 4.0 | 否 | 必须 | 仅限许可允许的非商用范围 | **排除**，不能用于计划商用的产品 |

CC BY 4.0 的官方摘要要求提供适当署名、许可链接并说明是否修改；官方进一步解释，适当署名可能包括作者、版权声明、许可声明、免责声明和素材链接。许可还禁止附加会限制许可权利的法律条款或有效技术措施。[CC BY 4.0 官方说明](https://creativecommons.org/licenses/by/4.0/deed.en) [Creative Commons DRM FAQ](https://creativecommons.org/faq/#can-i-use-effective-technological-measures-such-as-drm-when-i-share-cc-licensed-material)

CC0 和 CC BY 都不提供保证，也不一定覆盖商标、隐私、肖像、人格或其他权利。因此，提示音应优先选择作者明确说明为自行合成、且不含人声、品牌采样或影视/游戏片段的素材。[CC0 1.0 官方说明](https://creativecommons.org/publicdomain/zero/1.0/deed.en) [CC BY 4.0 官方说明](https://creativecommons.org/licenses/by/4.0/deed.en)

## 来源评估

### Kenney：最适合直接入库

[Interface Sounds](https://kenney.nl/assets/interface-sounds) 的作者官方页面明确列出 100 个文件和 CC0；[UI Audio](https://kenney.nl/assets/ui-audio) 明确列出 50 个文件和 CC0。两包下载压缩档内也附有 `License.txt`。来源集中、音色成套、无需署名，适合把选中的少量文件直接放进开源仓库。

建议保留包内 `License.txt`，并在项目的第三方素材清单中记录：包名、作者 Kenney、原始页面、所选文件名、下载日期和 CC0 链接。CC0 不强制这样做，但来源留证能应对网页变更和后续资产替换。

### Freesound：候选丰富，但逐文件核验

Freesound 允许上传者在 CC0、CC BY 和 CC BY-NC 中选择。其官方 FAQ 明确说明：CC0 基本可自由使用，CC BY 必须署名，CC BY-NC 不能用于营利作品；站内也提供许可过滤器和署名清单。[Freesound 许可 FAQ](https://freesound.org/help/faq/#what-do-i-need-to-do-to-legally-use-the-files-on-freesound)

平台同时提醒，内容由用户上传，仍可能存在上传者无权许可的材料。对 `beep_up.wav` / `beep_down.wav`，素材页称其由作者用 Audacity 制作，风险比来源不明的采样低；下载时仍应保存页面、原始 WAV 和许可信息。[beep_up.wav](https://freesound.org/people/paep3nguin/sounds/388046/) [beep_down.wav](https://freesound.org/people/paep3nguin/sounds/388047/) [Freesound 许可 FAQ](https://freesound.org/help/faq/#what-do-i-need-to-do-to-legally-use-the-files-on-freesound)

可用搜索方式：关键词用 `ui beep`、`toggle on off`、`record start stop`，许可只选 `Creative Commons 0`，时长限制为 0 至 1 秒，再人工组成音色一致的开始/结束对。Freesound 的“Free Cultural Works”过滤会同时包含 CC0 和 CC BY；若不准备维护署名，不要用这个较宽的过滤器。[Freesound 搜索](https://freesound.org/search/)

## 入库与试听标准

1. 只比较成对音效：开始为短促上行或较亮，结束为同音色的下行或较暗；让用户无需看屏幕也能区分状态。
2. 目标时长约 100 至 300 毫秒，避免长尾、语音、报警感、胜利音效和高频刺耳点击。先在 MacBook 扬声器、耳机及低系统音量下试听。
3. 选中后可裁掉多余静音、统一峰值并转为应用实际支持的格式；若使用 CC BY，素材清单必须标明这些修改。
4. 在资产旁保留许可文件，并记录原始 URL、作者、原文件名、下载日期、许可、修改步骤和文件哈希。不要依赖素材页永久在线。
5. 不把“royalty-free”“免费商用”自动等同于开放许可。对于采用自定义许可的素材站，除非逐条确认其许可允许随公开源码分发，否则不要把文件提交到仓库。

## 一手来源

- [Creative Commons：CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/deed.en)、[CC BY 4.0](https://creativecommons.org/licenses/by/4.0/deed.en) 与 [DRM FAQ](https://creativecommons.org/faq/#can-i-use-effective-technological-measures-such-as-drm-when-i-share-cc-licensed-material)
- [Kenney Interface Sounds](https://kenney.nl/assets/interface-sounds) 与 [Kenney UI Audio](https://kenney.nl/assets/ui-audio)
- [Freesound 许可 FAQ](https://freesound.org/help/faq/#licenses)
- [Freesound beep_up.wav](https://freesound.org/people/paep3nguin/sounds/388046/)
- [Freesound beep_down.wav](https://freesound.org/people/paep3nguin/sounds/388047/)
