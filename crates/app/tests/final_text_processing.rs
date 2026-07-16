use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use template_app::{
    FinalTextProcessor, FinalTextRequest, LlmProvider, LlmProviderError, LlmRefinementRequest,
    ProcessedText, RefinementFallbackReason, RefinementMode, RefinementStatus, RefinementTerm,
    normalize_standard_spellings, standard_spelling_occurs,
};
use tokio_util::sync::CancellationToken;

struct CountingProvider {
    calls: AtomicUsize,
    result: Result<String, LlmProviderError>,
}

#[async_trait]
impl LlmProvider for CountingProvider {
    async fn refine(&self, _request: LlmRefinementRequest) -> Result<String, LlmProviderError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.result.clone()
    }
}

struct PendingProvider;

struct CapturingProvider {
    request: std::sync::Mutex<Option<LlmRefinementRequest>>,
    result: String,
}

#[async_trait]
impl LlmProvider for CapturingProvider {
    async fn refine(&self, request: LlmRefinementRequest) -> Result<String, LlmProviderError> {
        let mut captured = self
            .request
            .lock()
            .map_err(|_| LlmProviderError::Transport("capture lock failed".to_owned()))?;
        *captured = Some(request);
        Ok(self.result.clone())
    }
}

#[async_trait]
impl LlmProvider for PendingProvider {
    async fn refine(&self, _request: LlmRefinementRequest) -> Result<String, LlmProviderError> {
        std::future::pending().await
    }
}

fn enabled_request(transcript: &str) -> FinalTextRequest {
    FinalTextRequest::new(transcript, RefinementMode::Enabled)
}

fn eligible_transcript(transcript: &str) -> String {
    format!("{transcript} 这是一段需要经过完整润色流程处理的较长语音内容。")
}

fn eligible_candidate(candidate: &str) -> String {
    if candidate.is_empty() {
        String::new()
    } else {
        eligible_transcript(candidate)
    }
}

#[tokio::test]
async fn disabled_refinement_returns_safely_cleaned_text_without_calling_provider() {
    let provider = Arc::new(CountingProvider {
        calls: AtomicUsize::new(0),
        result: Ok("unused".to_owned()),
    });
    let processor = FinalTextProcessor::configured(provider.clone());

    let result = processor
        .process(
            FinalTextRequest::new("  hello   world  \n  next line  ", RefinementMode::Disabled),
            CancellationToken::new(),
        )
        .await;

    assert_eq!(
        Ok(ProcessedText {
            text: "hello world\nnext line".to_owned(),
            refinement: RefinementStatus::Disabled,
        }),
        result
    );
    assert_eq!(0, provider.calls.load(Ordering::Relaxed));
}

#[tokio::test]
async fn disabled_refinement_does_not_infer_split_words() {
    let processor = FinalTextProcessor::unconfigured();
    let mut request = FinalTextRequest::new("please use say more today", RefinementMode::Disabled);
    request.relevant_terms = vec![RefinementTerm {
        canonical: "Saymore".to_owned(),
    }];

    let result = processor.process(request, CancellationToken::new()).await;

    assert_eq!(
        Ok(ProcessedText {
            text: "please use say more today".to_owned(),
            refinement: RefinementStatus::Disabled,
        }),
        result
    );
}

#[test]
fn standard_spellings_normalize_case_and_full_width_tokens() {
    let terms = vec![
        RefinementTerm {
            canonical: "OpenAI".to_owned(),
        },
        RefinementTerm {
            canonical: "GitHub".to_owned(),
        },
    ];

    assert_eq!(
        "OpenAI、OpenAI、OpenAI 和 GitHub",
        normalize_standard_spellings("openai、OPENAI、ＯｐｅｎＡＩ 和 github", &terms)
    );
    assert!(standard_spelling_occurs("使用openai接口", "OpenAI"));
}

#[test]
fn standard_spellings_do_not_join_words_or_rewrite_protected_tokens() {
    let terms = vec![RefinementTerm {
        canonical: "OpenAI".to_owned(),
    }];
    let text = "open ai openair myopenai https://openai.com me@openai.com openai_client";

    assert_eq!(text, normalize_standard_spellings(text, &terms));
}

#[tokio::test]
async fn ambiguous_cjk_substrings_are_not_replaced_locally() {
    let processor = FinalTextProcessor::unconfigured();
    let mut request = FinalTextRequest::new("我喝苹果汁", RefinementMode::Disabled);
    request.relevant_terms = vec![RefinementTerm {
        canonical: "Apple".to_owned(),
    }];

    let result = processor.process(request, CancellationToken::new()).await;

    assert_eq!(
        Ok(ProcessedText {
            text: "我喝苹果汁".to_owned(),
            refinement: RefinementStatus::Disabled,
        }),
        result
    );
}

#[tokio::test]
async fn enabled_refinement_returns_one_provider_result() {
    let provider = Arc::new(CountingProvider {
        calls: AtomicUsize::new(0),
        result: Ok("Clean text.".to_owned()),
    });
    let processor = FinalTextProcessor::configured(provider.clone());
    let attempts = AtomicUsize::new(0);

    let result = processor
        .process_with_attempt_observer(
            FinalTextRequest::new(
                "Today we need to finish the login test and then review the release settings",
                RefinementMode::Enabled,
            ),
            CancellationToken::new(),
            || {
                attempts.fetch_add(1, Ordering::Relaxed);
            },
        )
        .await;

    assert_eq!(
        Ok(ProcessedText {
            text: "Clean text.".to_owned(),
            refinement: RefinementStatus::Completed,
        }),
        result
    );
    assert_eq!(1, attempts.load(Ordering::Relaxed));
    assert_eq!(1, provider.calls.load(Ordering::Relaxed));
}

#[tokio::test]
async fn enabled_refinement_skips_short_transcripts_without_calling_provider() {
    let provider = Arc::new(CountingProvider {
        calls: AtomicUsize::new(0),
        result: Ok("unused".to_owned()),
    });
    let processor = FinalTextProcessor::configured(provider.clone());
    let attempts = AtomicUsize::new(0);

    for transcript in [
        "不是周三，是周四。",
        "第一步修复登录。",
        "我想要今天下午去。",
    ] {
        assert_eq!(
            Ok(ProcessedText {
                text: transcript.to_owned(),
                refinement: RefinementStatus::Skipped(
                    template_app::RefinementSkipReason::ShortTranscript,
                ),
            }),
            processor
                .process_with_attempt_observer(
                    enabled_request(transcript),
                    CancellationToken::new(),
                    || {
                        attempts.fetch_add(1, Ordering::Relaxed);
                    },
                )
                .await
        );
    }
    assert_eq!(0, attempts.load(Ordering::Relaxed));
    assert_eq!(0, provider.calls.load(Ordering::Relaxed));
}

#[tokio::test]
async fn twelve_cjk_units_are_refined_when_llm_is_enabled() {
    let provider = Arc::new(CountingProvider {
        calls: AtomicUsize::new(0),
        result: Ok("咱们接下来就先完成这个吧？".to_owned()),
    });
    let processor = FinalTextProcessor::configured(provider.clone());

    let result = processor
        .process(
            enabled_request("咱们接下来就完成这个吧先？"),
            CancellationToken::new(),
        )
        .await;

    assert_eq!(
        Ok(ProcessedText {
            text: "咱们接下来就先完成这个吧？".to_owned(),
            refinement: RefinementStatus::Completed,
        }),
        result
    );
    assert_eq!(1, provider.calls.load(Ordering::Relaxed));
}

#[tokio::test]
async fn evaluation_can_force_a_short_transcript_through_the_provider() {
    let provider = Arc::new(CountingProvider {
        calls: AtomicUsize::new(0),
        result: Ok("好的，我知道了。".to_owned()),
    });
    let processor = FinalTextProcessor::configured(provider.clone());

    let result = processor
        .evaluate(
            enabled_request("好的我知道了"),
            CancellationToken::new(),
            template_app::RefinementEvaluationMode::ForceProvider,
        )
        .await;

    assert_eq!(1, provider.calls.load(Ordering::Relaxed));
    assert_eq!(
        Ok(template_app::RefinementEvaluation {
            processed: ProcessedText {
                text: "好的，我知道了。".to_owned(),
                refinement: RefinementStatus::Completed,
            },
            provider_output: Some("好的，我知道了。".to_owned()),
        }),
        result
    );
}

#[tokio::test]
async fn evaluation_preserves_a_candidate_rejected_by_the_output_guard() {
    let transcript = eligible_transcript("会议安排在三点。");
    let candidate = eligible_candidate("会议安排在四点。");
    let processor = FinalTextProcessor::configured(Arc::new(CountingProvider {
        calls: AtomicUsize::new(0),
        result: Ok(candidate.clone()),
    }));

    let result = processor
        .evaluate(
            enabled_request(&transcript),
            CancellationToken::new(),
            template_app::RefinementEvaluationMode::ProductionPolicy,
        )
        .await;

    assert_eq!(
        Ok(template_app::RefinementEvaluation {
            processed: ProcessedText {
                text: transcript,
                refinement: RefinementStatus::FellBack(RefinementFallbackReason::OutputRejected,),
            },
            provider_output: Some(candidate),
        }),
        result
    );
}

#[test]
fn refinement_threshold_uses_cjk_units_and_english_words() {
    assert!(!template_app::refinement_needed(
        "一二三四五六七八九十甲",
        &RefinementMode::Enabled,
    ));
    assert!(template_app::refinement_needed(
        "一二三四五六七八九十甲乙",
        &RefinementMode::Enabled,
    ));
    assert!(!template_app::refinement_needed(
        "one two three four five six seven",
        &RefinementMode::Enabled,
    ));
    assert!(template_app::refinement_needed(
        "one two three four five six seven eight",
        &RefinementMode::Enabled,
    ));
    assert!(!template_app::refinement_needed(
        "I'm sure it's fine, don't worry",
        &RefinementMode::Enabled,
    ));
    assert!(!template_app::refinement_needed(
        "A well-tested end-to-end flow isn't broken",
        &RefinementMode::Enabled,
    ));
}

#[tokio::test]
async fn provider_receives_the_fixed_conservative_policy_and_relevant_context()
-> Result<(), Box<dyn std::error::Error>> {
    let source = eligible_transcript("请在 github 仓库里继续测试。");
    let refined = eligible_transcript("请在 GitHub 仓库里继续测试。");
    let provider = Arc::new(CapturingProvider {
        request: std::sync::Mutex::new(None),
        result: refined.clone(),
    });
    let processor = FinalTextProcessor::configured(provider.clone());
    let mut request = enabled_request(&source);
    request.language = Some("zh-CN".to_owned());
    request.relevant_terms = vec![RefinementTerm {
        canonical: "GitHub".to_owned(),
    }];

    let result = processor.process(request, CancellationToken::new()).await?;
    let captured = provider
        .request
        .lock()
        .map_err(|_| "capture lock failed")?
        .clone()
        .ok_or("provider request was not captured")?;

    let expected_terms = vec![RefinementTerm {
        canonical: "GitHub".to_owned(),
    }];
    if result.text != refined
        || captured.transcript != source
        || captured.language != Some("zh-CN".to_owned())
        || captured.relevant_terms != expected_terms
        || !captured
            .instructions
            .contains("Preserve information and communicative intent")
        || !captured
            .instructions
            .contains("A question stays a question")
        || !captured.instructions.contains("Preserve hierarchy")
        || !captured
            .instructions
            .contains("If brevity conflicts with coverage, keep the extra words")
    {
        return Err("provider did not receive the expected refinement context".into());
    }
    Ok(())
}

#[tokio::test]
async fn conservative_transformations_pass_the_output_guard() {
    let cases = [
        (
            "会议安排在周三，不对，周四下午三点。",
            "会议安排在周四下午三点。",
        ),
        (
            "我准备下午三点，不对，下午五点左右去公园玩。",
            "我准备下午五点左右去公园玩。",
        ),
        ("不要周三，改成周四。", "周四。"),
        ("会议安排在下午三点。", "会议安排在下午 3 点。"),
        ("版本二零二六。", "版本 2026。"),
        ("会议安排在十二点。", "会议安排在 12 点。"),
        ("一百零二个样本。", "102 个样本。"),
        ("会议在三小时后开始。", "会议在 3 小时后开始。"),
        ("还有十二公里。", "还有 12 公里。"),
        ("这个答案不对。", "这个答案不对。"),
        ("不是周三，是周四。", "是周四。"),
        (
            "要做两步，第一打开设置，第二选择模型。",
            "要做两步：\n1. 打开设置\n2. 选择模型。",
        ),
        (
            "模型返回以后，我们再把结果如果检查通过的话保存到历史里面。",
            "模型返回以后，如果检查通过的话，我们再把结果保存到历史里面。",
        ),
        (
            "我现在用的是 table 这个语音输入软件。",
            "我现在用的是 Typeless 这个语音输入软件。",
        ),
        (
            "请访问 https://example.com/v1，版本是 v1.2.3。",
            "请访问 https://example.com/v1，版本是 v1.2.3。",
        ),
    ];

    for (source, refined) in cases {
        let source = eligible_transcript(source);
        let refined = eligible_candidate(refined);
        let processor = FinalTextProcessor::configured(Arc::new(CountingProvider {
            calls: AtomicUsize::new(0),
            result: Ok(refined.clone()),
        }));
        let result = processor
            .process(enabled_request(&source), CancellationToken::new())
            .await;
        assert_eq!(
            Ok(ProcessedText {
                text: refined,
                refinement: RefinementStatus::Completed,
            }),
            result,
            "safe transformation was rejected for {source}"
        );
    }
}

#[tokio::test]
async fn standard_spelling_terms_allow_exact_format_corrections() {
    let cases = [
        (
            "请在 github 仓库里运行 cargo test --workspace，然后把结果贴到 notion。",
            "请在 GitHub 仓库里运行 cargo test --workspace，然后把结果贴到 Notion。",
            vec![
                RefinementTerm {
                    canonical: "GitHub".to_owned(),
                },
                RefinementTerm {
                    canonical: "Notion".to_owned(),
                },
            ],
        ),
        (
            "请把代码放到 Git Hub。",
            "请把代码放到 GitHub。",
            vec![RefinementTerm {
                canonical: "GitHub".to_owned(),
            }],
        ),
    ];

    for (source, refined, relevant_terms) in cases {
        let source = eligible_transcript(source);
        let refined = eligible_candidate(refined);
        let processor = FinalTextProcessor::configured(Arc::new(CountingProvider {
            calls: AtomicUsize::new(0),
            result: Ok(refined.clone()),
        }));
        let mut request = enabled_request(&source);
        request.relevant_terms = relevant_terms;

        assert_eq!(
            Ok(ProcessedText {
                text: refined,
                refinement: RefinementStatus::Completed,
            }),
            processor.process(request, CancellationToken::new()).await,
            "confirmed term correction was rejected for {source}"
        );
    }
}

#[tokio::test]
async fn standard_spelling_terms_do_not_authorize_other_technical_changes() {
    let cases = [
        (
            "请在 github 仓库里运行 cargo test --workspace。",
            "请在 GitHub 仓库里运行 cargo check --workspace。",
        ),
        (
            "请访问 https://github.com/example。",
            "请访问 https://GitHub.com/example。",
        ),
        ("这个 timetable 需要更新。", "这个 timeTypeless 需要更新。"),
    ];
    let relevant_terms = vec![
        RefinementTerm {
            canonical: "GitHub".to_owned(),
        },
        RefinementTerm {
            canonical: "Typeless".to_owned(),
        },
    ];

    for (source, unsafe_output) in cases {
        let source = eligible_transcript(source);
        let unsafe_output = eligible_candidate(unsafe_output);
        let processor = FinalTextProcessor::configured(Arc::new(CountingProvider {
            calls: AtomicUsize::new(0),
            result: Ok(unsafe_output),
        }));
        let mut request = enabled_request(&source);
        request.relevant_terms = relevant_terms.clone();

        assert_eq!(
            Ok(ProcessedText {
                text: source.clone(),
                refinement: RefinementStatus::FellBack(RefinementFallbackReason::OutputRejected),
            }),
            processor.process(request, CancellationToken::new()).await,
            "unconfirmed technical change was accepted for {source}"
        );
    }
}

#[tokio::test]
async fn unsafe_provider_outputs_fall_back_to_the_cleaned_transcript() {
    let expanded = "新增内容".repeat(40);
    let cases = [
        ("保留这句话。", ""),
        ("保留这句话。", expanded.as_str()),
        ("保留这句话。", "润色结果：保留这句话。"),
        ("版本是 v1.2.3。", "版本是 v1.2.4。"),
        ("会议安排在 3 点。", "会议安排在 4 点。"),
        ("会议安排在 3 点。", "会议安排在 3 点，提醒 4 点。"),
        ("会议安排在 3 点。", "会议安排在 3 点，提醒四点。"),
        ("会议安排在三点。", "会议安排在四点。"),
        (
            "我准备下午三点去，准备下午五点左右去公园玩。",
            "我准备下午五点左右去公园玩。",
        ),
        (
            "预算三十元，会议下午三点，不对，下午五点开始。",
            "会议下午五点开始。",
        ),
        ("会议在三小时后开始。", "会议在四小时后开始。"),
        ("万一失败怎么办？", "10001 失败怎么办？"),
        ("有一点担心。", "有 1 点担心。"),
        (
            "这不是问题，稍后再说，是另一个话题。",
            "这是问题，稍后再说，是另一个话题。",
        ),
        ("这不是问题，是答案。", "这是问题，是答案。"),
        (
            "请访问 https://example.com/v1。",
            "请访问 https://example.com/v2。",
        ),
        ("邮箱是 me@example.com。", "邮箱是 other@example.com。"),
        ("文件在 /tmp/demo.rs。", "文件在 /tmp/final.rs。"),
        (
            "请运行 cargo test --workspace。",
            "请运行 cargo check --workspace。",
        ),
        ("请运行 cargo test。", "请运行 cargo check。"),
        ("这个功能不能删除。", "这个功能可以删除。"),
        ("你觉得这个方案能不能实现？", "这个方案可以实现。"),
        ("类型是 FinalTextProcessor。", "类型是 TextProcessor。"),
        ("保留这句话。", "# 润色文本\n\n保留这句话。"),
    ];

    for (source, unsafe_output) in cases {
        let source = eligible_transcript(source);
        let unsafe_output = eligible_candidate(unsafe_output);
        let processor = FinalTextProcessor::configured(Arc::new(CountingProvider {
            calls: AtomicUsize::new(0),
            result: Ok(unsafe_output),
        }));
        let result = processor
            .process(enabled_request(&source), CancellationToken::new())
            .await;
        assert_eq!(
            Ok(ProcessedText {
                text: source.clone(),
                refinement: RefinementStatus::FellBack(RefinementFallbackReason::OutputRejected,),
            }),
            result,
            "unsafe output was accepted for {source}"
        );
    }
}

#[tokio::test]
async fn provider_failure_falls_back_to_safely_cleaned_text() {
    let provider = Arc::new(CountingProvider {
        calls: AtomicUsize::new(0),
        result: Err(LlmProviderError::Transport("offline".to_owned())),
    });
    let processor = FinalTextProcessor::configured(provider);
    let transcript = "keep this transcript exactly as it was originally spoken by the user";

    let result = processor
        .process(
            FinalTextRequest::new(transcript, RefinementMode::Enabled),
            CancellationToken::new(),
        )
        .await;

    assert_eq!(
        Ok(ProcessedText {
            text: transcript.to_owned(),
            refinement: RefinementStatus::FellBack(RefinementFallbackReason::Transport),
        }),
        result
    );
}

#[tokio::test(start_paused = true)]
async fn provider_request_times_out_after_eight_seconds() {
    let processor = FinalTextProcessor::configured(Arc::new(PendingProvider));
    let transcript = "keep this transcript exactly as it was originally spoken by the user";

    let result = processor
        .process(enabled_request(transcript), CancellationToken::new())
        .await;

    assert_eq!(
        Ok(ProcessedText {
            text: transcript.to_owned(),
            refinement: RefinementStatus::FellBack(RefinementFallbackReason::Timeout),
        }),
        result
    );
}

#[tokio::test]
async fn cancellation_prevents_a_late_refinement_result() {
    let processor = Arc::new(FinalTextProcessor::configured(Arc::new(PendingProvider)));
    let cancellation = CancellationToken::new();
    let task_processor = Arc::clone(&processor);
    let task_cancellation = cancellation.clone();
    let task = tokio::spawn(async move {
        task_processor
            .process(
                enabled_request(
                    "discard this transcript because the user cancelled the complete request",
                ),
                task_cancellation,
            )
            .await
    });
    tokio::task::yield_now().await;

    cancellation.cancel();

    assert!(matches!(
        task.await,
        Ok(Err(template_app::FinalTextProcessingError::Cancelled))
    ));
}

#[tokio::test]
async fn cancellation_also_stops_disabled_and_unconfigured_paths() {
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let disabled = FinalTextProcessor::unconfigured()
        .process(
            FinalTextRequest::new("discard this", RefinementMode::Disabled),
            cancellation.clone(),
        )
        .await;
    let unconfigured = FinalTextProcessor::unconfigured()
        .process(enabled_request("discard this"), cancellation)
        .await;

    assert_eq!(
        Err(template_app::FinalTextProcessingError::Cancelled),
        disabled
    );
    assert_eq!(
        Err(template_app::FinalTextProcessingError::Cancelled),
        unconfigured
    );
}

#[tokio::test(start_paused = true)]
async fn three_transient_failures_pause_calls_for_five_minutes() {
    let provider = Arc::new(CountingProvider {
        calls: AtomicUsize::new(0),
        result: Err(LlmProviderError::Transport("offline".to_owned())),
    });
    let processor = FinalTextProcessor::configured(provider.clone());
    let cancellation = CancellationToken::new();
    let transcript = "keep this transcript exactly as it was originally spoken by the user";

    for _ in 0..3 {
        let _ = processor
            .process(enabled_request(transcript), cancellation.clone())
            .await;
    }
    let paused = processor
        .process(enabled_request(transcript), cancellation.clone())
        .await;

    assert_eq!(
        Ok(ProcessedText {
            text: transcript.to_owned(),
            refinement: RefinementStatus::FellBack(
                RefinementFallbackReason::TemporarilyUnavailable,
            ),
        }),
        paused
    );
    assert_eq!(3, provider.calls.load(Ordering::Relaxed));

    tokio::time::advance(std::time::Duration::from_secs(300)).await;
    let _ = processor
        .process(enabled_request(transcript), cancellation)
        .await;
    assert_eq!(4, provider.calls.load(Ordering::Relaxed));
}
