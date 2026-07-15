# Local Voice Evaluation

This evaluation set is the shared baseline for ASR, LLM refinement, dictionary
normalization, and final delivery. Read each script naturally instead of pasting
the text. The expected result is the benchmark answer; Dev history supplies the
actual ASR, LLM, and final texts.

## Run Protocol

1. Run only `Saymore Preview` so the global shortcut is owned by one process.
2. Confirm the Dev dictionary contains `OpenAI`, `GitHub`, `SQLite`, `DeepSeek`,
   `SenseNova`, and `Saymore` with language `zh-Hans`.
3. Run the full set with SenseNova, then switch to DeepSeek and run it again.
4. Speak at a natural pace. Do not deliberately dictate punctuation.
5. For dictionary cases D01-D03, also run once with LLM refinement disabled.
6. Compare Dev history's ASR, LLM, and final texts with the expected result.
7. Group at least five similar failures before changing a prompt or local rule.

## Guided Capture Session

There are two different capture sessions. Choose based on whether the audio must
be reusable.

### Manual Baseline Available Now

This session records ASR, LLM, and final text in Dev history, but it does not
save audio. The same script must be read again when switching Providers.

1. Open a plain text target such as TextEdit and place the cursor in an empty
   document.
2. Keep this evaluation document open beside Saymore Preview.
3. Select SenseNova and confirm refinement is enabled.
4. Start recording with the right Command key.
5. Read only the quoted `Read` text for R01 at a natural pace.
6. Stop recording and wait until the final text is delivered.
7. Open Saymore's newest history item and inspect its three stages.
8. Compare those stages with R01's `Expected` text and assign one result class.
9. Continue in case-ID order through P01.
10. Switch to DeepSeek, clear the target document, and repeat the same order.
11. Disable refinement and repeat D01-D03 to isolate local normalization.

During a guided session, report `R01 completed`, `R02 completed`, and so on.
Codex can inspect the newest Dev history item and maintain the result record;
the speaker does not need to retype the three-stage texts manually.

### Reusable Audio Dataset

Start the local recorder with `node tests/voice-evaluation/server.mjs`, then open
`http://127.0.0.1:4173`. The page shows one case at a time, records normalized
16 kHz mono PCM WAV, allows playback before acceptance, and advances after the
clip is saved. Files stay under `tests/voice-evaluation/recordings/<case-id>/`
and are ignored by Git. Record each case once in a quiet environment with the
same microphone and normal speaking pace.

The canonical 23-case manifest is `tests/voice-evaluation/cases.json`. It
contains the cases below plus W01-W08, which isolate topic fronting, displaced
objects, time placement, conditions, parallel modifiers, ambiguous scope,
negation, and inserted clauses.

## Evaluation Set

### Routing And Fidelity

#### R01 - Short Transcript Bypass

Read:

> 好的，我知道了

Expected:

> 好的，我知道了。

Check: the result is delivered promptly and does not wait for an LLM request.

#### R02 - Emphatic Repetition

Read:

> 这个真的真的很重要，我们今天必须先验证完整流程，然后才能决定是否发布

Expected:

> 这个真的真的很重要，我们今天必须先验证完整流程，然后才能决定是否发布。

Check: meaningful emphasis is retained.

#### R03 - Question Is Not Answered

Read:

> 我想确认一下明天下午几点开会，你先帮我记录这个问题，我稍后自己去确认

Expected:

> 我想确认一下明天下午几点开会。你先帮我记录这个问题，我稍后自己去确认。

Check: the LLM formats the question but does not answer it.

### Cleanup And Correction

#### C01 - Stutter And Accidental Repetition

Read:

> 我我我觉得这个这个功能很好，所以我们可以先完成测试，然后再决定是否发布

Expected:

> 我觉得这个功能很好，所以我们可以先完成测试，然后再决定是否发布。

#### C02 - Empty Fillers

Read:

> 嗯，明天下午开会，呃，主要讨论登录问题和设置页面，最后确认发布计划

Expected:

> 明天下午开会，主要讨论登录问题和设置页面，最后确认发布计划。

#### C03 - Explicit Self-Correction

Read:

> 会议安排在周三，不对，周四下午三点，届时我们会检查登录问题和发布计划

Expected:

> 会议安排在周四下午三点，届时我们会检查登录问题和发布计划。

#### C04 - Conservative Casual Wording

Read:

> 这个事情就是他们那边还没给，然后我们这边现在就弄不了，只能等他们给了以后再继续

Expected:

> 这个事情他们那边还没给，我们这边现在就弄不了，只能等他们给了以后再继续。

Check: the meaning and casual tone are retained; avoid formal rewriting.

### Structure And Punctuation

#### S01 - Numbered Steps

Read:

> 接下来要做三步，第一检查登录配置，第二测试语音输入，第三确认发布文档

Expected:

```text
接下来要做三步：

1. 检查登录配置
2. 测试语音输入
3. 确认发布文档
```

#### S02 - Time-Based Paragraphs

Read:

> 今天先把登录问题修好并完成测试，明天再处理设置页面的样式，发布之前还需要检查一次配置迁移，避免老用户的数据丢失

Expected:

```text
今天先把登录问题修好并完成测试。

明天再处理设置页面的样式。

发布之前还需要检查一次配置迁移，避免老用户的数据丢失。
```

#### S03 - Cause And Result Stay Together

Read:

> 这个功能失败是因为当前网络不稳定，所以我们先保留原来的处理方式，等网络恢复以后再重新测试

Expected:

> 这个功能失败是因为当前网络不稳定，所以我们先保留原来的处理方式，等网络恢复以后再重新测试。

Check: cause and result remain in one paragraph.

#### S04 - Unfinished Final Unit

Read:

> 今天先完成登录测试，明天处理设置页面，然后发布之前我还想要

Expected:

```text
今天先完成登录测试。

明天处理设置页面。

然后发布之前我还想要
```

Check: the unfinished ending is not completed and has no final punctuation.

### Dictionary And Mixed Language

#### D01 - OpenAI And DeepSeek

Read:

> 我准备先用 open ai 测试润色效果，再切换到 deep seek 比较同一段文本

Expected:

> 我准备先用 OpenAI 测试润色效果，再切换到 DeepSeek 比较同一段文本。

Check: if ASR returns `open ai` with a space, record it as an unsupported alias
case rather than silently treating the local normalizer as successful.

#### D02 - Saymore And SQLite

Read:

> saymore 使用 sqlite 保存本地历史，并且只在开发环境记录中间结果

Expected:

> Saymore 使用 SQLite 保存本地历史，并且只在开发环境记录中间结果。

#### D03 - SenseNova And GitHub

Read:

> sensenova 完成润色以后，我会把测试结果整理到 github

Expected:

> SenseNova 完成润色以后，我会把测试结果整理到 GitHub。

### Protected Content

#### P01 - URL Version And Command

Read:

> 请访问 https://example.com/v1，确认版本是 v1.2.3，然后运行 cargo test --workspace 并记录结果

Expected:

> 请访问 https://example.com/v1，确认版本是 v1.2.3，然后运行 cargo test --workspace 并记录结果。

Check: URL, version, and command remain byte-for-byte unchanged.

## Result Record

For each run, record:

```text
case_id:
provider:
ASR text:
LLM text:
final text:
expected text:
refinement status:
result: pass | asr_failure | llm_failure | normalization_failure | delivery_failure
notes:
```

The expected text comes from this evaluation set. A separate expected-text
field in ordinary user history is not required for this controlled test.

## Recorded-Audio Automation

The local evaluation recorder persists only explicitly accepted benchmark WAVs;
production history still does not save audio. The next evaluator slice can run
without a new desktop UI:

1. Traverse the canonical manifest and its saved WAV files.
2. Replay every WAV through the existing streaming ASR port in fixed-size chunks.
3. Save each ASR transcript as an immutable run artifact.
4. Send the same transcript to DeepSeek and SenseNova independently.
5. Save each provider output beside the expected text without overwriting WAVs.
6. Produce per-category results for ASR, word order, cleanup, structure, terms,
   and protected content.
5. Run the frozen transcript through every configured LLM Provider for a fair
   LLM-only comparison.
6. Optionally replay the WAV through ASR on every run for an end-to-end test.
7. Compare each stage with the expected text and emit a compact local report.

Two modes are needed because they answer different questions:

- LLM comparison freezes one ASR transcript and isolates Provider/prompt quality.
- End-to-end comparison replays audio and measures ASR, LLM, normalization, and
  delivery together.

Audio recording and reports remain local for the MVP. Cloud synchronization is
outside this evaluation contract.
