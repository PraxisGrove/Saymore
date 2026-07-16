use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
};

use rusqlite::{Connection, OpenFlags};
use template_app::{
    DictionaryCandidateEvidence, DictionaryEntry, DictionaryLearningOutcome,
    DictionaryLearningStore, DictionaryStore, HistoryCursor, HistoryPage, HistoryStore,
    InstalledModel, InstalledModelStore, LocalSettings, LocalSettingsStore, NewDictionaryEntry,
    NewDictionaryObservation, NewHistoryRecord, SecretStore, StorageError,
};

mod dictionary;
mod dictionary_learning;
mod history;
mod history_search;
mod migrations;
mod models;
mod settings;

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

impl HistoryStore for SqliteStorage {
    fn insert_history(&self, record: NewHistoryRecord) -> Result<(), StorageError> {
        self.request(|response| Command::InsertHistory { record, response })
    }

    fn history_page(
        &self,
        cursor: Option<HistoryCursor>,
        limit: u16,
    ) -> Result<HistoryPage, StorageError> {
        self.request(|response| Command::HistoryPage {
            cursor,
            limit,
            response,
        })
    }

    fn search_history_page(
        &self,
        cursor: Option<HistoryCursor>,
        limit: u16,
        query: &str,
    ) -> Result<HistoryPage, StorageError> {
        self.request(|response| Command::SearchHistoryPage {
            cursor,
            limit,
            query: query.to_owned(),
            response,
        })
    }

    fn delete_history(&self, id: &str) -> Result<(), StorageError> {
        self.request(|response| Command::DeleteHistory {
            id: id.to_owned(),
            response,
        })
    }

    fn update_history_delivery(
        &self,
        id: &str,
        delivery: template_app::HistoryDelivery,
    ) -> Result<(), StorageError> {
        self.request(|response| Command::UpdateHistoryDelivery {
            id: id.to_owned(),
            delivery,
            response,
        })
    }

    fn clear_history(&self) -> Result<(), StorageError> {
        self.request(Command::ClearHistory)
    }

    fn reset_history(&self) -> Result<(), StorageError> {
        self.request(Command::ResetHistory)
    }

    fn cleanup_history(&self, now_ms: i64) -> Result<u64, StorageError> {
        self.request(|response| Command::CleanupHistory { now_ms, response })
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

enum Command {
    LoadSettings(SyncSender<Result<LocalSettings, StorageError>>),
    SaveSettings {
        settings: LocalSettings,
        response: SyncSender<Result<(), StorageError>>,
    },
    InsertHistory {
        record: NewHistoryRecord,
        response: SyncSender<Result<(), StorageError>>,
    },
    HistoryPage {
        cursor: Option<HistoryCursor>,
        limit: u16,
        response: SyncSender<Result<HistoryPage, StorageError>>,
    },
    SearchHistoryPage {
        cursor: Option<HistoryCursor>,
        limit: u16,
        query: String,
        response: SyncSender<Result<HistoryPage, StorageError>>,
    },
    DeleteHistory {
        id: String,
        response: SyncSender<Result<(), StorageError>>,
    },
    UpdateHistoryDelivery {
        id: String,
        delivery: template_app::HistoryDelivery,
        response: SyncSender<Result<(), StorageError>>,
    },
    ClearHistory(SyncSender<Result<(), StorageError>>),
    ResetHistory(SyncSender<Result<(), StorageError>>),
    CleanupHistory {
        now_ms: i64,
        response: SyncSender<Result<u64, StorageError>>,
    },
    ListDictionary(SyncSender<Result<Vec<DictionaryEntry>, StorageError>>),
    UpsertDictionary {
        entry: NewDictionaryEntry,
        now_ms: i64,
        response: SyncSender<Result<DictionaryEntry, StorageError>>,
    },
    DeleteDictionary {
        id: String,
        response: SyncSender<Result<(), StorageError>>,
    },
    RecordDictionaryObservation {
        observation: NewDictionaryObservation,
        response: SyncSender<Result<DictionaryLearningOutcome, StorageError>>,
    },
    ListDictionaryCandidateEvidence(
        SyncSender<Result<Vec<DictionaryCandidateEvidence>, StorageError>>,
    ),
    ListInstalledModels(SyncSender<Result<Vec<InstalledModel>, StorageError>>),
    SaveInstalledModel {
        model: InstalledModel,
        response: SyncSender<Result<(), StorageError>>,
    },
    Shutdown,
}

pub(super) struct Database {
    connection: Connection,
    history_key: history::HistoryKeyState,
    secrets: Arc<dyn SecretStore>,
}

fn run_worker(
    path: PathBuf,
    secrets: Arc<dyn SecretStore>,
    receiver: Receiver<Command>,
    ready: SyncSender<Result<(), StorageError>>,
) {
    let database = open_database(path, secrets);
    if let Err(error) = &database {
        let _ = ready.send(Err(error.clone()));
        return;
    }
    let Ok(mut database) = database else {
        return;
    };
    if ready.send(Ok(())).is_err() {
        return;
    }
    for command in receiver {
        if !process_command(&mut database, command) {
            break;
        }
    }
}

fn process_command(database: &mut Database, command: Command) -> bool {
    match command {
        Command::LoadSettings(response) => {
            send_result(response, settings::load(&database.connection))
        }
        Command::SaveSettings { settings, response } => {
            let result = settings::save(&mut database.connection, &settings).and_then(|()| {
                history::cleanup(&mut database.connection, history::now_ms()).map(|_| ())
            });
            send_result(response, result)
        }
        Command::InsertHistory { record, response } => {
            send_result(response, history::insert(database, record))
        }
        Command::HistoryPage {
            cursor,
            limit,
            response,
        } => send_result(response, history::page(database, cursor, limit)),
        Command::SearchHistoryPage {
            cursor,
            limit,
            query,
            response,
        } => send_result(
            response,
            history_search::page(database, cursor, limit, &query),
        ),
        Command::DeleteHistory { id, response } => {
            send_result(response, history::delete(&mut database.connection, &id))
        }
        Command::UpdateHistoryDelivery {
            id,
            delivery,
            response,
        } => send_result(response, history::update_delivery(database, &id, delivery)),
        Command::ClearHistory(response) => {
            send_result(response, history::clear(&mut database.connection))
        }
        Command::ResetHistory(response) => send_result(response, history::reset(database)),
        Command::CleanupHistory { now_ms, response } => {
            send_result(response, history::cleanup(&mut database.connection, now_ms))
        }
        Command::ListDictionary(response) => {
            send_result(response, dictionary::list(&database.connection))
        }
        Command::UpsertDictionary {
            entry,
            now_ms,
            response,
        } => send_result(
            response,
            dictionary::upsert(&mut database.connection, entry, now_ms),
        ),
        Command::DeleteDictionary { id, response } => {
            send_result(response, dictionary::delete(&mut database.connection, &id))
        }
        Command::RecordDictionaryObservation {
            observation,
            response,
        } => send_result(
            response,
            dictionary_learning::record(&mut database.connection, observation),
        ),
        Command::ListDictionaryCandidateEvidence(response) => send_result(
            response,
            dictionary_learning::list_evidence(&database.connection),
        ),
        Command::ListInstalledModels(response) => {
            send_result(response, models::list(&database.connection))
        }
        Command::SaveInstalledModel { model, response } => {
            send_result(response, models::save(&mut database.connection, model))
        }
        Command::Shutdown => return false,
    }
    true
}

fn send_result<T>(response: SyncSender<Result<T, StorageError>>, result: Result<T, StorageError>) {
    let _ = response.send(result);
}

fn open_database(path: PathBuf, secrets: Arc<dyn SecretStore>) -> Result<Database, StorageError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(unavailable)?;
    }
    let mut connection = Connection::open(path).map_err(unavailable)?;
    connection
        .execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;
             PRAGMA synchronous = FULL;
             PRAGMA busy_timeout = 3000;
             PRAGMA secure_delete = ON;",
        )
        .map_err(unavailable)?;
    let integrity: String = connection
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .map_err(unavailable)?;
    if integrity != "ok" {
        return Err(StorageError::Invalid(format!(
            "SQLite integrity check failed: {integrity}"
        )));
    }
    migrations::apply(&mut connection)?;
    Ok(Database {
        connection,
        history_key: history::HistoryKeyState::Uninitialized,
        secrets,
    })
}

pub(super) fn unavailable(error: impl std::fmt::Display) -> StorageError {
    StorageError::Unavailable(error.to_string())
}

pub(super) fn invalid(error: impl std::fmt::Display) -> StorageError {
    StorageError::Invalid(error.to_string())
}
