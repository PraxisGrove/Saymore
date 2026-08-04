use std::sync::{Arc, Mutex};

use slint::{ComponentHandle, SharedString};
use template_app::DictionaryStore;
use template_infra::SqliteStorage;

use super::{
    dictionary_ui::{self, DictionaryUiState},
    now_ms, spawn_named,
};
use crate::ui::{AppWindow, Translations};

pub(super) fn wire(
    ui: &AppWindow,
    storage: Arc<SqliteStorage>,
    state: Arc<Mutex<DictionaryUiState>>,
) {
    wire_open(ui);
    wire_close(ui);
    wire_save(ui, storage, state);
}

fn wire_open(ui: &AppWindow) {
    let weak_ui = ui.as_weak();
    ui.on_open_dictionary_edit(move |id, canonical| {
        if let Some(ui) = weak_ui.upgrade() {
            ui.set_dictionary_edit_id(id);
            ui.set_dictionary_edit_original_value(canonical.clone());
            ui.set_dictionary_edit_value(canonical);
            ui.set_dictionary_edit_failed(false);
            ui.set_dictionary_status(SharedString::new());
            ui.set_dictionary_edit_visible(true);
        }
    });
}

fn wire_close(ui: &AppWindow) {
    let weak_ui = ui.as_weak();
    ui.on_close_dictionary_edit(move || {
        let Some(ui) = weak_ui.upgrade() else {
            return;
        };
        if ui.get_dictionary_edit_saving() {
            return;
        }
        clear_dialog(&ui);
        ui.set_dictionary_status(SharedString::new());
    });
}

fn wire_save(ui: &AppWindow, storage: Arc<SqliteStorage>, state: Arc<Mutex<DictionaryUiState>>) {
    let weak_ui = ui.as_weak();
    ui.on_save_dictionary_edit(move |id, canonical| {
        let canonical = canonical.trim().to_owned();
        if canonical.is_empty() {
            if let Some(ui) = weak_ui.upgrade() {
                show_edit_error(&ui);
            }
            return;
        }
        if let Some(ui) = weak_ui.upgrade() {
            ui.set_dictionary_edit_saving(true);
            ui.set_dictionary_edit_failed(false);
            ui.set_dictionary_status(SharedString::new());
        }
        save_async(
            weak_ui.clone(),
            Arc::clone(&storage),
            Arc::clone(&state),
            id.to_string(),
            canonical,
        );
    });
}

fn save_async(
    ui: slint::Weak<AppWindow>,
    storage: Arc<SqliteStorage>,
    state: Arc<Mutex<DictionaryUiState>>,
    id: String,
    canonical: String,
) {
    spawn_named("saymore-edit-dictionary", move || {
        let result = storage.update_dictionary(&id, &canonical, now_ms());
        let entries = storage.list_dictionary();
        let _ = ui.upgrade_in_event_loop(move |ui| {
            ui.set_dictionary_edit_saving(false);
            match (result, entries) {
                (Ok(_), Ok(entries)) => {
                    dictionary_ui::replace_entries(&ui, &state, entries);
                    clear_dialog(&ui);
                    ui.set_dictionary_status(
                        ui.global::<Translations>().get_dictionary_entry_updated(),
                    );
                }
                (Err(error), _) => {
                    tracing::warn!(event = "dictionary.edit_failed", reason = %error);
                    show_edit_error(&ui);
                }
                (_, Err(error)) => {
                    tracing::warn!(event = "dictionary.reload_after_edit_failed", reason = %error);
                    ui.set_dictionary_edit_failed(true);
                    ui.set_dictionary_status(ui.global::<Translations>().get_storage_error());
                }
            }
        });
    });
}

fn show_edit_error(ui: &AppWindow) {
    ui.set_dictionary_edit_failed(true);
    ui.set_dictionary_status(ui.global::<Translations>().get_dictionary_edit_failed());
}

fn clear_dialog(ui: &AppWindow) {
    ui.set_dictionary_edit_visible(false);
    ui.set_dictionary_edit_failed(false);
    ui.set_dictionary_edit_id(SharedString::new());
    ui.set_dictionary_edit_original_value(SharedString::new());
    ui.set_dictionary_edit_value(SharedString::new());
}
