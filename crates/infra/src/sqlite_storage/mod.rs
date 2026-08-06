use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        mpsc::{self, SyncSender},
    },
    thread::{self, JoinHandle},
};

use rusqlite::{Connection, OpenFlags};
use template_app::{
    DiagnosticEventStore, DictationHistoryWriter, DictionaryCandidateEvidence, DictionaryEntry,
    DictionaryLearningOutcome, DictionaryLearningStore, DictionaryStore, HistoryCursor,
    HistoryPage, HistoryStore, InstalledModel, InstalledModelStore, LocalSettings,
    LocalSettingsStore, ModelDownloadQueueStore, NewDictionaryEntry, NewDictionaryObservation,
    NewHistoryRecord, QueuedModelDownload, SecretStore, StorageError,
};

mod diagnostics;
mod dictionary;
mod dictionary_learning;
mod history;
mod history_search;
mod migrations;
mod model_download_queue;
mod models;
mod settings;
mod usage;
mod worker;

use worker::{Command, Database, HistoryCommand, run as run_worker};

const QUEUE_CAPACITY: usize = 64;

pub struct SqliteStorage {
    commands: SyncSender<Command>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl SqliteStorage {
    pub fn start(path: PathBuf, secrets: Arc<dyn SecretStore>) -> Result<Self, StorageError> {
        let (commands, receiver) = mpsc::sync_channel(QUEUE_CAPACITY);
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("saymore-sqlite".to_owned())
            .spawn(move || run_worker(path, secrets, receiver, ready_sender))
            .map_err(unavailable)?;
        match ready_receiver.recv() {
            Ok(Ok(())) => Ok(Self {
                commands,
                worker: Mutex::new(Some(worker)),
            }),
            Ok(Err(error)) => {
                let _ = worker.join();
                Err(error)
            }
            Err(error) => {
                let _ = worker.join();
                Err(unavailable(error))
            }
        }
    }

    fn request<T>(
        &self,
        build: impl FnOnce(SyncSender<Result<T, StorageError>>) -> Command,
    ) -> Result<T, StorageError> {
        let (sender, receiver) = mpsc::sync_channel(1);
        self.commands.send(build(sender)).map_err(unavailable)?;
        receiver.recv().map_err(unavailable)?
    }
}

/// Reads a stable dictionary snapshot without starting the writable storage worker.
///
/// Evaluation and diagnostics callers use this to avoid migrations, history-key
/// access, and competing writers while the desktop application is running.
pub fn read_dictionary_snapshot(path: &Path) -> Result<Vec<DictionaryEntry>, StorageError> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(unavailable)?;
    dictionary::list(&connection)
}

impl LocalSettingsStore for SqliteStorage {
    fn load_settings(&self) -> Result<LocalSettings, StorageError> {
        self.request(Command::LoadSettings)
    }

    fn save_settings(&self, settings: LocalSettings) -> Result<(), StorageError> {
        self.request(|response| Command::SaveSettings { settings, response })
    }
}

impl template_app::VocabularySuggestionSettingsStore for SqliteStorage {
    fn record_vocabulary_suggestion_success(
        &self,
        consent_fingerprint: &str,
        completed_at_ms: i64,
    ) -> Result<bool, StorageError> {
        self.request(|response| Command::RecordVocabularySuggestionSuccess {
            consent_fingerprint: consent_fingerprint.to_owned(),
            completed_at_ms,
            response,
        })
    }
}

impl DiagnosticEventStore for SqliteStorage {
    fn record_diagnostic_event(&self, event: &str) -> Result<(), StorageError> {
        self.request(|response| Command::RecordDiagnosticEvent {
            event: event.to_owned(),
            response,
        })
    }

    fn diagnostic_events(&self, limit: u32) -> Result<Vec<String>, StorageError> {
        self.request(|response| Command::DiagnosticEvents { limit, response })
    }
}

impl HistoryStore for SqliteStorage {
    fn insert_history(&self, record: NewHistoryRecord) -> Result<(), StorageError> {
        self.request(|response| Command::History(HistoryCommand::Insert { record, response }))
    }

    fn update_history_final_text(&self, id: &str, final_text: &str) -> Result<(), StorageError> {
        self.request(|response| {
            Command::History(HistoryCommand::UpdateFinalText {
                id: id.to_owned(),
                final_text: final_text.to_owned(),
                response,
            })
        })
    }

    fn history_page(
        &self,
        cursor: Option<HistoryCursor>,
        limit: u16,
    ) -> Result<HistoryPage, StorageError> {
        self.request(|response| {
            Command::History(HistoryCommand::Page {
                cursor,
                limit,
                response,
            })
        })
    }

    fn search_history_page(
        &self,
        cursor: Option<HistoryCursor>,
        limit: u16,
        query: &str,
    ) -> Result<HistoryPage, StorageError> {
        self.request(|response| {
            Command::History(HistoryCommand::SearchPage {
                cursor,
                limit,
                query: query.to_owned(),
                response,
            })
        })
    }

    fn delete_history(&self, id: &str) -> Result<(), StorageError> {
        self.request(|response| {
            Command::History(HistoryCommand::Delete {
                id: id.to_owned(),
                response,
            })
        })
    }

    fn update_history_delivery(
        &self,
        id: &str,
        delivery: template_app::HistoryDelivery,
    ) -> Result<(), StorageError> {
        self.request(|response| {
            Command::History(HistoryCommand::UpdateDelivery {
                id: id.to_owned(),
                delivery,
                response,
            })
        })
    }

    fn clear_history(&self) -> Result<(), StorageError> {
        self.request(|response| Command::History(HistoryCommand::Clear(response)))
    }

    fn reset_history(&self) -> Result<(), StorageError> {
        self.request(|response| Command::History(HistoryCommand::Reset(response)))
    }

    fn cleanup_history(&self, now_ms: i64) -> Result<u64, StorageError> {
        self.request(|response| Command::History(HistoryCommand::Cleanup { now_ms, response }))
    }
}

impl DictationHistoryWriter for SqliteStorage {
    fn insert_history(&self, record: NewHistoryRecord) -> Result<(), StorageError> {
        HistoryStore::insert_history(self, record)
    }
}

impl DictionaryStore for SqliteStorage {
    fn list_dictionary(&self) -> Result<Vec<DictionaryEntry>, StorageError> {
        self.request(Command::ListDictionary)
    }

    fn upsert_dictionary(
        &self,
        entry: NewDictionaryEntry,
        now_ms: i64,
    ) -> Result<DictionaryEntry, StorageError> {
        self.request(|response| Command::UpsertDictionary {
            entry,
            now_ms,
            response,
        })
    }

    fn update_dictionary(
        &self,
        id: &str,
        canonical: &str,
        now_ms: i64,
    ) -> Result<DictionaryEntry, StorageError> {
        self.request(|response| Command::UpdateDictionary {
            id: id.to_owned(),
            canonical: canonical.to_owned(),
            now_ms,
            response,
        })
    }

    fn delete_dictionary(&self, id: &str) -> Result<(), StorageError> {
        self.request(|response| Command::DeleteDictionary {
            id: id.to_owned(),
            response,
        })
    }
}

impl DictionaryLearningStore for SqliteStorage {
    fn record_dictionary_observation(
        &self,
        observation: NewDictionaryObservation,
    ) -> Result<DictionaryLearningOutcome, StorageError> {
        self.request(|response| Command::RecordDictionaryObservation {
            observation,
            response,
        })
    }

    fn list_dictionary_candidate_evidence(
        &self,
    ) -> Result<Vec<DictionaryCandidateEvidence>, StorageError> {
        self.request(Command::ListDictionaryCandidateEvidence)
    }
}

impl InstalledModelStore for SqliteStorage {
    fn list_installed_models(&self) -> Result<Vec<InstalledModel>, StorageError> {
        self.request(Command::ListInstalledModels)
    }

    fn save_installed_model(&self, model: InstalledModel) -> Result<(), StorageError> {
        self.request(|response| Command::SaveInstalledModel { model, response })
    }

    fn delete_installed_model(&self, id: &str) -> Result<(), StorageError> {
        self.request(|response| Command::DeleteInstalledModel {
            id: id.to_owned(),
            response,
        })
    }
}

impl ModelDownloadQueueStore for SqliteStorage {
    fn queued_model_downloads(&self) -> Result<Vec<QueuedModelDownload>, StorageError> {
        self.request(Command::ListModelDownloadQueue)
    }

    fn enqueue_model_download(&self, model_id: &str) -> Result<(), StorageError> {
        self.request(|response| Command::EnqueueModelDownload {
            model_id: model_id.to_owned(),
            response,
        })
    }

    fn remove_model_download(&self, model_id: &str) -> Result<(), StorageError> {
        self.request(|response| Command::RemoveModelDownload {
            model_id: model_id.to_owned(),
            response,
        })
    }
}

impl Drop for SqliteStorage {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Ok(mut worker) = self.worker.lock()
            && let Some(worker) = worker.take()
        {
            let _ = worker.join();
        }
    }
}

pub(super) fn unavailable(error: impl std::fmt::Display) -> StorageError {
    StorageError::Unavailable(error.to_string())
}

pub(super) fn invalid(error: impl std::fmt::Display) -> StorageError {
    StorageError::Invalid(error.to_string())
}
