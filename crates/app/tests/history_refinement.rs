use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use template_app::{
    DictionaryEntry, DictionaryOrigin, DictionaryStore, FinalTextProcessor, FinalTextRequest,
    HistoryCursor, HistoryDelivery, HistoryPage, HistoryRefinementRetry,
    HistoryRefinementRetryError, HistoryRefinementUpdate, HistoryStore, LlmProvider,
    LlmProviderError, LlmRefinementRequest, NewDictionaryEntry, NewHistoryRecord,
    RefinementFallbackReason, RefinementMode, RefinementStatus, RefinementStrength, StorageError,
    retry_history_refinement,
};
use tokio_util::sync::CancellationToken;

const TRANSCRIPT: &str = "嗯那个我们明天下午三点开会讨论一下 openai 的接入方案吧";

struct ScriptedProvider {
    calls: AtomicUsize,
    requests: Mutex<Vec<LlmRefinementRequest>>,
    result: Result<String, LlmProviderError>,
}

impl ScriptedProvider {
    fn new(result: Result<String, LlmProviderError>) -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            requests: Mutex::new(Vec::new()),
            result,
        })
    }
}

#[async_trait]
impl LlmProvider for ScriptedProvider {
    async fn refine(&self, request: LlmRefinementRequest) -> Result<String, LlmProviderError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(request);
        self.result.clone()
    }
}

#[derive(Default)]
struct Stores {
    dictionary: Vec<DictionaryEntry>,
    updates: Mutex<Vec<(String, HistoryRefinementUpdate)>>,
}

impl DictionaryStore for Stores {
    fn list_dictionary(&self) -> Result<Vec<DictionaryEntry>, StorageError> {
        Ok(self.dictionary.clone())
    }

    fn upsert_dictionary(
        &self,
        _entry: NewDictionaryEntry,
        _now_ms: i64,
    ) -> Result<DictionaryEntry, StorageError> {
        Err(unexpected())
    }

    fn update_dictionary(
        &self,
        _id: &str,
        _canonical: &str,
        _now_ms: i64,
    ) -> Result<DictionaryEntry, StorageError> {
        Err(unexpected())
    }

    fn delete_dictionary(&self, _id: &str) -> Result<(), StorageError> {
        Err(unexpected())
    }
}

impl HistoryStore for Stores {
    fn insert_history(&self, _record: NewHistoryRecord) -> Result<(), StorageError> {
        Err(unexpected())
    }

    fn update_history_final_text(&self, _id: &str, _final_text: &str) -> Result<(), StorageError> {
        Err(unexpected())
    }

    fn record_history_refinement(
        &self,
        id: &str,
        update: HistoryRefinementUpdate,
    ) -> Result<(), StorageError> {
        self.updates
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((id.to_owned(), update));
        Ok(())
    }

    fn history_page(
        &self,
        _cursor: Option<HistoryCursor>,
        _limit: u16,
    ) -> Result<HistoryPage, StorageError> {
        Err(unexpected())
    }

    fn search_history_page(
        &self,
        _cursor: Option<HistoryCursor>,
        _limit: u16,
        _query: &str,
    ) -> Result<HistoryPage, StorageError> {
        Err(unexpected())
    }

    fn update_history_delivery(
        &self,
        _id: &str,
        _delivery: HistoryDelivery,
    ) -> Result<(), StorageError> {
        Err(unexpected())
    }

    fn delete_history(&self, _id: &str) -> Result<(), StorageError> {
        Err(unexpected())
    }

    fn clear_history(&self) -> Result<(), StorageError> {
        Err(unexpected())
    }

    fn reset_history(&self) -> Result<(), StorageError> {
        Err(unexpected())
    }

    fn cleanup_history(&self, _now_ms: i64) -> Result<u64, StorageError> {
        Err(unexpected())
    }
}

fn unexpected() -> StorageError {
    StorageError::Unavailable("unexpected storage call".to_owned())
}

fn retry(transcript: &str) -> HistoryRefinementRetry {
    HistoryRefinementRetry {
        history_id: "history-1".to_owned(),
        transcript: transcript.to_owned(),
        strength: RefinementStrength::Thorough,
        llm_provider_id: Some("deepseek".to_owned()),
        llm_model: Some("deepseek-flash".to_owned()),
    }
}

fn updates(stores: &Stores) -> Vec<(String, HistoryRefinementUpdate)> {
    stores
        .updates
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

#[tokio::test]
async fn successful_retry_saves_and_returns_normalized_polished_text() {
    let provider =
        ScriptedProvider::new(Ok("明天下午三点开会，讨论 openai 的接入方案。".to_owned()));
    let processor = FinalTextProcessor::configured(provider.clone());
    let stores = Stores {
        dictionary: vec![DictionaryEntry {
            id: "openai".to_owned(),
            canonical: "OpenAI".to_owned(),
            language: "en".to_owned(),
            origin: DictionaryOrigin::Manual,
            created_at_ms: 1,
            updated_at_ms: 1,
        }],
        ..Stores::default()
    };

    let result = retry_history_refinement(
        &processor,
        &stores,
        &stores,
        retry(TRANSCRIPT),
        CancellationToken::new(),
    )
    .await;

    let expected = "明天下午三点开会，讨论 OpenAI 的接入方案。";
    assert_eq!(Ok(expected.to_owned()), result);
    assert_eq!(
        vec![(
            "history-1".to_owned(),
            HistoryRefinementUpdate {
                refined_text: expected.to_owned(),
                llm_provider_id: Some("deepseek".to_owned()),
                llm_model: Some("deepseek-flash".to_owned()),
            }
        )],
        updates(&stores)
    );
    let requests = provider
        .requests
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    assert_eq!(1, requests.len());
    assert!(requests.iter().all(|request| {
        request
            .instructions
            .contains("Polishing strength: thorough.")
    }));
}

#[tokio::test]
async fn failed_retry_reports_the_reason_without_changing_history() {
    let processor = FinalTextProcessor::configured(ScriptedProvider::new(Err(
        LlmProviderError::Transport("offline".to_owned()),
    )));
    let stores = Stores::default();

    let result = retry_history_refinement(
        &processor,
        &stores,
        &stores,
        retry(TRANSCRIPT),
        CancellationToken::new(),
    )
    .await;

    assert_eq!(
        Err(HistoryRefinementRetryError::NotRefined(
            RefinementStatus::FellBack(RefinementFallbackReason::Transport)
        )),
        result
    );
    assert!(updates(&stores).is_empty());
}

#[tokio::test]
async fn empty_transcript_is_rejected_before_calling_the_provider() {
    let provider = ScriptedProvider::new(Ok("unused".to_owned()));
    let processor = FinalTextProcessor::configured(provider.clone());
    let stores = Stores::default();

    let result = retry_history_refinement(
        &processor,
        &stores,
        &stores,
        retry(" \n "),
        CancellationToken::new(),
    )
    .await;

    assert_eq!(Err(HistoryRefinementRetryError::EmptyTranscript), result);
    assert_eq!(0, provider.calls.load(Ordering::Relaxed));
}

#[tokio::test]
async fn user_retry_calls_the_provider_even_while_automatic_polishing_is_paused() {
    let provider = ScriptedProvider::new(Err(LlmProviderError::Transport("offline".to_owned())));
    let processor = FinalTextProcessor::configured(provider.clone());
    for _ in 0..3 {
        let _ = processor
            .process(
                FinalTextRequest::new(TRANSCRIPT, RefinementMode::Enabled),
                CancellationToken::new(),
            )
            .await;
    }
    let paused = processor
        .process(
            FinalTextRequest::new(TRANSCRIPT, RefinementMode::Enabled),
            CancellationToken::new(),
        )
        .await;
    assert_eq!(
        Ok(RefinementStatus::FellBack(
            RefinementFallbackReason::TemporarilyUnavailable
        )),
        paused.map(|processed| processed.refinement)
    );
    assert_eq!(3, provider.calls.load(Ordering::Relaxed));

    let stores = Stores::default();
    let _ = retry_history_refinement(
        &processor,
        &stores,
        &stores,
        retry("短句"),
        CancellationToken::new(),
    )
    .await;

    assert_eq!(4, provider.calls.load(Ordering::Relaxed));
}
