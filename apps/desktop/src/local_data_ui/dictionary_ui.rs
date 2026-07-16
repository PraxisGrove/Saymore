use std::sync::{Arc, Mutex};

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use template_app::{
    CandidateAssessmentSource, DictionaryCandidateEvidence, DictionaryCandidateKind,
    DictionaryCandidateState, DictionaryEntry, DictionaryLearningStore, DictionaryOrigin,
    DictionaryStore, NewDictionaryEntry,
};
use template_infra::{DictionaryFiles, SqliteStorage};

use crate::{
    local_data_ui::{now_ms, spawn_named},
    ui::{
        AppWindow, DictionaryDraft, DictionaryEvidenceItem, DictionaryFilterKind,
        DictionaryListItem, DictionaryOriginKind, DictionaryRow, Translations,
    },
};

pub(super) struct DictionaryUiState {
    entries: Vec<DictionaryEntry>,
    evidence: Vec<DictionaryCandidateEvidence>,
    filter: DictionaryFilterKind,
    query: String,
    drafts: Vec<Draft>,
    layout_width: f32,
}

impl Default for DictionaryUiState {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            evidence: Vec::new(),
            filter: DictionaryFilterKind::All,
            query: String::new(),
            drafts: Vec::new(),
            layout_width: 0.0,
        }
    }
}

#[derive(Default)]
struct Draft {
    value: String,
    focused: bool,
}

pub(super) fn load_initial(
    ui: &AppWindow,
    storage: &SqliteStorage,
    state: &Arc<Mutex<DictionaryUiState>>,
) {
    match storage.list_dictionary() {
        Ok(entries) => {
            if let Ok(mut state) = state.lock() {
                state.entries = entries;
                state.evidence = storage
                    .list_dictionary_candidate_evidence()
                    .unwrap_or_default();
                apply_state(ui, &state);
            }
        }
        Err(error) => {
            ui.set_dictionary_load_failed(true);
            tracing::warn!(event = "dictionary.load_failed", reason = %error);
            ui.set_dictionary_status(ui.global::<Translations>().get_storage_error());
        }
    }
}

pub(super) fn wire(
    ui: &AppWindow,
    storage: Arc<SqliteStorage>,
    state: Arc<Mutex<DictionaryUiState>>,
) {
    wire_refresh(ui, Arc::clone(&storage), Arc::clone(&state));
    wire_filter_and_search(ui, Arc::clone(&state));
    wire_draft_actions(ui, Arc::clone(&storage), Arc::clone(&state));
    wire_csv_import(ui, Arc::clone(&storage), Arc::clone(&state));
    wire_delete(ui, storage, state);
}

fn wire_csv_import(
    ui: &AppWindow,
    storage: Arc<SqliteStorage>,
    state: Arc<Mutex<DictionaryUiState>>,
) {
    let import_ui = ui.as_weak();
    ui.on_import_dictionary_csv(move || {
        let Some(window) = import_ui.upgrade() else {
            return;
        };
        let path = rfd::FileDialog::new()
            .add_filter("CSV", &["csv"])
            .pick_file();
        let Some(path) = path else {
            return;
        };
        window.set_dictionary_importing(true);
        window.set_dictionary_status(SharedString::new());

        let ui = import_ui.clone();
        let storage = Arc::clone(&storage);
        let state = Arc::clone(&state);
        spawn_named("saymore-import-dictionary", move || {
            let dictionary_store: Arc<dyn DictionaryStore> = storage.clone();
            let result =
                DictionaryFiles::new(dictionary_store).import_csv(&path, "zh-Hans", now_ms());
            let entries = storage.list_dictionary();
            let evidence = storage.list_dictionary_candidate_evidence();
            let _ = ui.upgrade_in_event_loop(move |window| {
                window.set_dictionary_importing(false);
                match (result, entries) {
                    (Ok(report), Ok(entries)) => {
                        if let Ok(mut state) = state.lock() {
                            state.entries = entries;
                            if let Ok(evidence) = evidence {
                                state.evidence = evidence;
                            }
                            apply_state(&window, &state);
                        }
                        window.set_dictionary_status(
                            window.global::<Translations>().invoke_dictionary_imported(
                                i32::try_from(report.added).unwrap_or(i32::MAX),
                                i32::try_from(report.skipped).unwrap_or(i32::MAX),
                            ),
                        );
                    }
                    (Err(error), _) => {
                        tracing::warn!(event = "dictionary.import_failed", reason = %error);
                        window.set_dictionary_status(
                            window
                                .global::<Translations>()
                                .get_dictionary_import_failed(),
                        );
                    }
                    (_, Err(error)) => {
                        tracing::warn!(event = "dictionary.import_failed", reason = %error);
                        window.set_dictionary_status(
                            window
                                .global::<Translations>()
                                .get_dictionary_import_failed(),
                        );
                    }
                }
            });
        });
    });
}

fn wire_refresh(ui: &AppWindow, storage: Arc<SqliteStorage>, state: Arc<Mutex<DictionaryUiState>>) {
    let weak_ui = ui.as_weak();
    ui.on_refresh_dictionary(move || {
        if let Some(ui) = weak_ui.upgrade() {
            ui.set_dictionary_loading(true);
            ui.set_dictionary_load_failed(false);
            ui.set_dictionary_status(SharedString::from(""));
        }
        let weak_ui = weak_ui.clone();
        let storage = Arc::clone(&storage);
        let state = Arc::clone(&state);
        spawn_named("saymore-refresh-dictionary", move || {
            let result = storage.list_dictionary();
            let evidence = storage.list_dictionary_candidate_evidence();
            let _ = weak_ui.upgrade_in_event_loop(move |ui| {
                ui.set_dictionary_loading(false);
                match result {
                    Ok(entries) => {
                        ui.set_dictionary_load_failed(false);
                        if let Ok(mut state) = state.lock() {
                            state.entries = entries;
                            if let Ok(evidence) = evidence {
                                state.evidence = evidence;
                            }
                            apply_state(&ui, &state);
                        }
                    }
                    Err(error) => {
                        ui.set_dictionary_load_failed(true);
                        tracing::warn!(event = "dictionary.refresh_failed", reason = %error);
                        ui.set_dictionary_status(ui.global::<Translations>().get_storage_error());
                    }
                }
            });
        });
    });
}

fn wire_filter_and_search(ui: &AppWindow, state: Arc<Mutex<DictionaryUiState>>) {
    let filter_ui = ui.as_weak();
    let filter_state = Arc::clone(&state);
    ui.on_filter_dictionary(move |filter| {
        if let Ok(mut state) = filter_state.lock() {
            state.filter = filter;
            if let Some(ui) = filter_ui.upgrade() {
                apply_state(&ui, &state);
            }
        }
    });

    let search_ui = ui.as_weak();
    let search_state = Arc::clone(&state);
    ui.on_search_dictionary(move |query| {
        if let Ok(mut state) = search_state.lock() {
            state.query = query.to_string();
            if let Some(ui) = search_ui.upgrade() {
                apply_state(&ui, &state);
            }
        }
    });

    let layout_ui = ui.as_weak();
    let layout_state = Arc::clone(&state);
    ui.on_set_dictionary_layout_width(move |width| {
        if let Ok(mut state) = layout_state.lock() {
            let width = width.max(1.0);
            if (state.layout_width - width).abs() > f32::EPSILON {
                state.layout_width = width;
                if let Some(ui) = layout_ui.upgrade() {
                    apply_state(&ui, &state);
                }
            }
        }
    });
}

fn wire_draft_actions(
    ui: &AppWindow,
    storage: Arc<SqliteStorage>,
    state: Arc<Mutex<DictionaryUiState>>,
) {
    wire_draft_dialog(ui, Arc::clone(&state));
    wire_draft_rows(ui, Arc::clone(&state));
    wire_draft_save(ui, storage, state);
}

fn wire_draft_dialog(ui: &AppWindow, state: Arc<Mutex<DictionaryUiState>>) {
    let open_ui = ui.as_weak();
    let open_state = Arc::clone(&state);
    ui.on_open_dictionary_add(move || {
        if let Ok(mut state) = open_state.lock() {
            state.drafts = vec![Draft {
                value: String::new(),
                focused: false,
            }];
            if let Some(ui) = open_ui.upgrade() {
                ui.set_dictionary_status(SharedString::from(""));
                ui.set_dictionary_save_failed(false);
                ui.set_dictionary_add_visible(true);
                apply_drafts(&ui, &state);
            }
        }
    });

    let close_ui = ui.as_weak();
    let close_state = Arc::clone(&state);
    ui.on_close_dictionary_add(move || {
        if let Some(ui) = close_ui.upgrade() {
            if ui.get_dictionary_saving() {
                return;
            }
            if let Ok(mut state) = close_state.lock() {
                state.drafts.clear();
            }
            ui.set_dictionary_add_visible(false);
            ui.set_dictionary_save_failed(false);
            ui.set_dictionary_status(SharedString::from(""));
        }
    });
}

fn wire_draft_rows(ui: &AppWindow, state: Arc<Mutex<DictionaryUiState>>) {
    let add_ui = ui.as_weak();
    let add_state = Arc::clone(&state);
    ui.on_add_dictionary_draft(move || {
        if let Ok(mut state) = add_state.lock() {
            for draft in &mut state.drafts {
                draft.focused = false;
            }
            state.drafts.push(Draft {
                value: String::new(),
                focused: true,
            });
            if let Some(ui) = add_ui.upgrade() {
                apply_drafts(&ui, &state);
            }
        }
    });

    let update_ui = ui.as_weak();
    let update_state = Arc::clone(&state);
    ui.on_update_dictionary_draft(move |index, value| {
        let Ok(index) = usize::try_from(index) else {
            return;
        };
        if let Ok(mut state) = update_state.lock()
            && let Some(draft) = state.drafts.get_mut(index)
        {
            draft.value = value.to_string();
            draft.focused = false;
            if let Some(ui) = update_ui.upgrade() {
                ui.set_dictionary_draft_nonempty_count(
                    state
                        .drafts
                        .iter()
                        .filter(|draft| !draft.value.trim().is_empty())
                        .count() as i32,
                );
            }
        }
    });

    let remove_ui = ui.as_weak();
    let remove_state = Arc::clone(&state);
    ui.on_remove_dictionary_draft(move |index| {
        let Ok(index) = usize::try_from(index) else {
            return;
        };
        if let Ok(mut state) = remove_state.lock()
            && state.drafts.len() > 1
            && index < state.drafts.len()
        {
            state.drafts.remove(index);
            if let Some(ui) = remove_ui.upgrade() {
                apply_drafts(&ui, &state);
            }
        }
    });
}

fn wire_draft_save(
    ui: &AppWindow,
    storage: Arc<SqliteStorage>,
    state: Arc<Mutex<DictionaryUiState>>,
) {
    let save_ui = ui.as_weak();
    ui.on_save_dictionary_drafts(move || {
        let words = match state.lock() {
            Ok(state) => state
                .drafts
                .iter()
                .map(|draft| draft.value.trim().to_owned())
                .filter(|word| !word.is_empty())
                .collect::<Vec<_>>(),
            Err(_) => Vec::new(),
        };
        if words.is_empty() {
            return;
        }
        if let Some(ui) = save_ui.upgrade() {
            ui.set_dictionary_saving(true);
            ui.set_dictionary_save_failed(false);
            ui.set_dictionary_status(SharedString::from(""));
        }
        save_drafts_async(
            save_ui.clone(),
            Arc::clone(&storage),
            Arc::clone(&state),
            words,
        );
    });
}

fn save_drafts_async(
    ui: slint::Weak<AppWindow>,
    storage: Arc<SqliteStorage>,
    state: Arc<Mutex<DictionaryUiState>>,
    words: Vec<String>,
) {
    spawn_named("saymore-save-dictionary", move || {
        let total = words.len();
        let result = words.into_iter().try_for_each(|word| {
            storage
                .upsert_dictionary(
                    NewDictionaryEntry {
                        canonical: word,
                        language: "zh-Hans".to_owned(),
                        origin: DictionaryOrigin::Manual,
                    },
                    now_ms(),
                )
                .map(|_| ())
        });
        let entries = storage.list_dictionary();
        let _ = ui.upgrade_in_event_loop(move |ui| {
            ui.set_dictionary_saving(false);
            match entries {
                Ok(entries) => {
                    if let Ok(mut state) = state.lock() {
                        state.entries = entries;
                        if result.is_ok() {
                            state.drafts.clear();
                            ui.set_dictionary_add_visible(false);
                            ui.set_dictionary_save_failed(false);
                            ui.set_dictionary_status(
                                ui.global::<Translations>().invoke_dictionary_saved(
                                    i32::try_from(total).unwrap_or(i32::MAX),
                                ),
                            );
                        } else {
                            ui.set_dictionary_save_failed(true);
                            ui.set_dictionary_status(
                                ui.global::<Translations>().get_dictionary_save_failed(),
                            );
                        }
                        apply_state(&ui, &state);
                    }
                }
                Err(error) => {
                    ui.set_dictionary_save_failed(true);
                    tracing::warn!(event = "dictionary.reload_after_save_failed", reason = %error);
                    ui.set_dictionary_status(ui.global::<Translations>().get_storage_error());
                }
            }
        });
    });
}

fn wire_delete(ui: &AppWindow, storage: Arc<SqliteStorage>, state: Arc<Mutex<DictionaryUiState>>) {
    let weak_ui = ui.as_weak();
    ui.on_delete_dictionary_word(move |id| {
        let ui = weak_ui.clone();
        let storage = Arc::clone(&storage);
        let state = Arc::clone(&state);
        spawn_named("saymore-delete-dictionary", move || {
            let result = storage.delete_dictionary(id.as_str());
            let entries = storage.list_dictionary();
            let _ = ui.upgrade_in_event_loop(move |ui| match (result, entries) {
                (Ok(()), Ok(entries)) => {
                    if let Ok(mut state) = state.lock() {
                        state.entries = entries;
                        apply_state(&ui, &state);
                    }
                    ui.set_dictionary_status(
                        ui.global::<Translations>().get_dictionary_entry_deleted(),
                    );
                }
                (Err(error), _) | (_, Err(error)) => {
                    tracing::warn!(event = "dictionary.delete_failed", reason = %error);
                    ui.set_dictionary_status(ui.global::<Translations>().get_storage_error());
                }
            });
        });
    });
}

fn apply_state(ui: &AppWindow, state: &DictionaryUiState) {
    let query = state.query.trim().to_lowercase();
    let entries = state
        .entries
        .iter()
        .filter(|entry| matches_filter(entry, &state.filter))
        .filter(|entry| query.is_empty() || entry.canonical.to_lowercase().contains(&query))
        .collect::<Vec<_>>();
    let items = entries
        .iter()
        .map(|entry| to_list_item(entry))
        .collect::<Vec<_>>();
    let rows = dictionary_rows(entries, state.layout_width);
    ui.set_dictionary_items(ModelRc::new(VecModel::from(items)));
    ui.set_dictionary_rows(ModelRc::new(VecModel::from(rows)));
    ui.set_dictionary_total_count(state.entries.len() as i32);
    ui.set_dictionary_evidence_items(ModelRc::new(VecModel::from(
        state
            .evidence
            .iter()
            .map(to_evidence_item)
            .collect::<Vec<_>>(),
    )));
    ui.set_dictionary_filter(state.filter);
    apply_drafts(ui, state);
}

fn to_evidence_item(evidence: &DictionaryCandidateEvidence) -> DictionaryEvidenceItem {
    let kind = match evidence.assessment.kind {
        DictionaryCandidateKind::NamedTerm => "named term",
        DictionaryCandidateKind::Acronym => "acronym",
        DictionaryCandidateKind::CodeIdentifier => "code identifier",
        DictionaryCandidateKind::ProfessionalPhrase => "professional phrase",
        DictionaryCandidateKind::OrdinaryFragment => "ordinary fragment",
        DictionaryCandidateKind::Unknown => "unknown",
    };
    let source = match evidence.assessment.source {
        CandidateAssessmentSource::Local => "local",
        CandidateAssessmentSource::Llm => "LLM",
    };
    let state = match evidence.state {
        DictionaryCandidateState::Pending => "pending",
        DictionaryCandidateState::Promoted => "promoted",
    };
    DictionaryEvidenceItem {
        canonical: evidence.canonical.as_str().into(),
        detail: format!(
            "{kind} · {source} · {}% · {} edits / {} dictations · {state}",
            evidence.assessment.confidence, evidence.occurrence_count, evidence.dictation_count
        )
        .into(),
    }
}

fn apply_drafts(ui: &AppWindow, state: &DictionaryUiState) {
    let drafts = state
        .drafts
        .iter()
        .map(|draft| DictionaryDraft {
            value: SharedString::from(draft.value.as_str()),
            focused: draft.focused,
        })
        .collect::<Vec<_>>();
    let nonempty_count = state
        .drafts
        .iter()
        .filter(|draft| !draft.value.trim().is_empty())
        .count() as i32;
    ui.set_dictionary_drafts(ModelRc::new(VecModel::from(drafts)));
    ui.set_dictionary_draft_nonempty_count(nonempty_count);
}

fn matches_filter(entry: &DictionaryEntry, filter: &DictionaryFilterKind) -> bool {
    match filter {
        DictionaryFilterKind::Automatic => entry.origin == DictionaryOrigin::Automatic,
        DictionaryFilterKind::Manual => entry.origin == DictionaryOrigin::Manual,
        DictionaryFilterKind::All => true,
    }
}

fn to_list_item(entry: &DictionaryEntry) -> DictionaryListItem {
    DictionaryListItem {
        id: SharedString::from(entry.id.as_str()),
        canonical: SharedString::from(entry.canonical.as_str()),
        language: SharedString::from(entry.language.as_str()),
        origin: match entry.origin {
            DictionaryOrigin::Manual => DictionaryOriginKind::Manual,
            DictionaryOrigin::Automatic => DictionaryOriginKind::Automatic,
        },
    }
}

fn dictionary_rows(entries: Vec<&DictionaryEntry>, layout_width: f32) -> Vec<DictionaryRow> {
    let available_width = layout_width.max(340.0) - 30.0;
    let mut rows = Vec::<DictionaryRow>::new();
    let mut row = Vec::<DictionaryListItem>::new();
    let mut used_width = 0.0_f32;
    for entry in entries {
        let item_width = dictionary_pill_width(&entry.canonical);
        let next_width = if row.is_empty() {
            item_width
        } else {
            used_width + 10.0 + item_width
        };
        if !row.is_empty() && next_width > available_width {
            rows.push(DictionaryRow {
                items: ModelRc::new(VecModel::from(row)),
            });
            row = Vec::new();
            used_width = 0.0;
        }
        used_width = if row.is_empty() {
            item_width
        } else {
            used_width + 10.0 + item_width
        };
        row.push(to_list_item(entry));
    }
    if !row.is_empty() {
        rows.push(DictionaryRow {
            items: ModelRc::new(VecModel::from(row)),
        });
    }
    rows
}

fn dictionary_pill_width(label: &str) -> f32 {
    let text_width = label.chars().fold(0.0, |width, character| {
        width + if character.is_ascii() { 7.5 } else { 13.0 }
    });
    text_width + 82.0
}
