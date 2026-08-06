use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        mpsc::{Receiver, SyncSender},
    },
};

use rusqlite::Connection;
use template_app::{
    DictionaryCandidateEvidence, DictionaryEntry, DictionaryLearningOutcome, HistoryCursor,
    HistoryPage, InstalledModel, LocalSettings, NewDictionaryEntry, NewDictionaryObservation,
    NewHistoryRecord, QueuedModelDownload, SecretStore, StorageError, UsageSnapshot,
};

use super::{
    diagnostics, dictionary, dictionary_learning, history, history_search, migrations,
    model_download_queue, models, settings, unavailable, usage,
};

pub(super) enum Command {
    LoadSettings(SyncSender<Result<LocalSettings, StorageError>>),
    SaveSettings {
        settings: LocalSettings,
        response: SyncSender<Result<(), StorageError>>,
    },
    RecordVocabularySuggestionSuccess {
        consent_fingerprint: String,
        completed_at_ms: i64,
        response: SyncSender<Result<bool, StorageError>>,
    },
    RecordDiagnosticEvent {
        event: String,
        response: SyncSender<Result<(), StorageError>>,
    },
    DiagnosticEvents {
        limit: u32,
        response: SyncSender<Result<Vec<String>, StorageError>>,
    },
    UsageSnapshot {
        period_start: chrono::NaiveDate,
        period_end: chrono::NaiveDate,
        response: SyncSender<Result<UsageSnapshot, StorageError>>,
    },
    History(HistoryCommand),
    ListDictionary(SyncSender<Result<Vec<DictionaryEntry>, StorageError>>),
    UpsertDictionary {
        entry: NewDictionaryEntry,
        now_ms: i64,
        response: SyncSender<Result<DictionaryEntry, StorageError>>,
    },
    UpdateDictionary {
        id: String,
        canonical: String,
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
    DeleteInstalledModel {
        id: String,
        response: SyncSender<Result<(), StorageError>>,
    },
    ListModelDownloadQueue(SyncSender<Result<Vec<QueuedModelDownload>, StorageError>>),
    EnqueueModelDownload {
        model_id: String,
        response: SyncSender<Result<(), StorageError>>,
    },
    RemoveModelDownload {
        model_id: String,
        response: SyncSender<Result<(), StorageError>>,
    },
    Shutdown,
}

pub(super) enum HistoryCommand {
    Insert {
        record: NewHistoryRecord,
        response: SyncSender<Result<(), StorageError>>,
    },
    Page {
        cursor: Option<HistoryCursor>,
        limit: u16,
        response: SyncSender<Result<HistoryPage, StorageError>>,
    },
    SearchPage {
        cursor: Option<HistoryCursor>,
        limit: u16,
        query: String,
        response: SyncSender<Result<HistoryPage, StorageError>>,
    },
    Delete {
        id: String,
        response: SyncSender<Result<(), StorageError>>,
    },
    UpdateDelivery {
        id: String,
        delivery: template_app::HistoryDelivery,
        response: SyncSender<Result<(), StorageError>>,
    },
    UpdateFinalText {
        id: String,
        final_text: String,
        response: SyncSender<Result<(), StorageError>>,
    },
    Clear(SyncSender<Result<(), StorageError>>),
    Reset(SyncSender<Result<(), StorageError>>),
    Cleanup {
        now_ms: i64,
        response: SyncSender<Result<u64, StorageError>>,
    },
}

pub(super) struct Database {
    pub(super) connection: Connection,
    pub(super) history_key: history::HistoryKeyState,
    pub(super) secrets: Arc<dyn SecretStore>,
}

pub(super) fn run(
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
            send_result(response, save_settings(database, &settings))
        }
        Command::RecordVocabularySuggestionSuccess {
            consent_fingerprint,
            completed_at_ms,
            response,
        } => send_result(
            response,
            settings::record_vocabulary_suggestion_success(
                &database.connection,
                &consent_fingerprint,
                completed_at_ms,
            ),
        ),
        Command::RecordDiagnosticEvent { event, response } => send_result(
            response,
            diagnostics::record(&mut database.connection, &event),
        ),
        Command::DiagnosticEvents { limit, response } => {
            send_result(response, diagnostics::list(&database.connection, limit))
        }
        Command::UsageSnapshot {
            period_start,
            period_end,
            response,
        } => send_result(
            response,
            usage::snapshot(database, period_start, period_end),
        ),
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
        Command::UpdateDictionary {
            id,
            canonical,
            now_ms,
            response,
        } => send_result(
            response,
            dictionary::update(&mut database.connection, &id, &canonical, now_ms),
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
        Command::DeleteInstalledModel { id, response } => {
            send_result(response, models::delete(&database.connection, &id))
        }
        Command::ListModelDownloadQueue(response) => {
            send_result(response, model_download_queue::list(&database.connection))
        }
        Command::EnqueueModelDownload { model_id, response } => send_result(
            response,
            model_download_queue::enqueue(&database.connection, &model_id),
        ),
        Command::RemoveModelDownload { model_id, response } => send_result(
            response,
            model_download_queue::remove(&database.connection, &model_id),
        ),
        Command::History(command) => process_history_command(database, command),
        Command::Shutdown => return false,
    }
    true
}

fn process_history_command(database: &mut Database, command: HistoryCommand) {
    match command {
        HistoryCommand::Insert { record, response } => {
            send_result(response, history::insert(database, record))
        }
        HistoryCommand::Page {
            cursor,
            limit,
            response,
        } => send_result(response, history::page(database, cursor, limit)),
        HistoryCommand::SearchPage {
            cursor,
            limit,
            query,
            response,
        } => send_result(
            response,
            history_search::page(database, cursor, limit, &query),
        ),
        HistoryCommand::Delete { id, response } => {
            let result = usage::backfill_history(database)
                .and_then(|()| history::delete(&mut database.connection, &id));
            send_result(response, result);
        }
        HistoryCommand::UpdateDelivery {
            id,
            delivery,
            response,
        } => send_result(response, history::update_delivery(database, &id, delivery)),
        HistoryCommand::UpdateFinalText {
            id,
            final_text,
            response,
        } => send_result(
            response,
            history::update_final_text(database, &id, &final_text),
        ),
        HistoryCommand::Clear(response) => {
            let result = usage::backfill_history(database)
                .and_then(|()| history::clear(&mut database.connection));
            send_result(response, result);
        }
        HistoryCommand::Reset(response) => send_result(response, history::reset(database)),
        HistoryCommand::Cleanup { now_ms, response } => {
            let result = usage::backfill_history(database)
                .and_then(|()| history::cleanup(&mut database.connection, now_ms));
            send_result(response, result);
        }
    }
}

fn save_settings(database: &mut Database, settings: &LocalSettings) -> Result<(), StorageError> {
    usage::backfill_history(database)?;
    settings::save(&mut database.connection, settings)
        .and_then(|()| history::cleanup(&mut database.connection, history::now_ms()).map(|_| ()))
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
