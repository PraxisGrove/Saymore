use super::*;

pub(super) fn wire(
    ui: &AppWindow,
    settings: Arc<JsonSettingsStore>,
    storage: Arc<SqliteStorage>,
    installers: Arc<LocalModelInstallers>,
) {
    let weak = ui.as_weak();
    ui.on_request_seaco_import(move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        let model = LocalModel::Seaco;
        if activation_pending(&ui, model) || model_selected(&ui, model) {
            return;
        }
        let Some(source) = rfd::FileDialog::new().pick_folder() else {
            return;
        };
        let installer = installers.for_model(model);
        let settings = Arc::clone(&settings);
        let storage = Arc::clone(&storage);
        set_activation_pending(&ui, model, true);
        ui.set_local_model_operation_error(SharedString::default());
        let result_ui = weak.clone();
        let spawn = std::thread::Builder::new()
            .name(model.thread_name("import"))
            .spawn(move || {
                let result = installer
                    .import_directory(&source)
                    .map_err(|error| error.to_string());
                let _ = result_ui.upgrade_in_event_loop(move |ui| {
                    set_activation_pending(&ui, model, false);
                    match result {
                        Ok(_) => {
                            if let Err(error) =
                                apply_state(&ui, &settings, &storage, model, &installer)
                            {
                                apply_activation_failure(&ui, model, error.to_string());
                            }
                            ui.invoke_refresh_usage();
                        }
                        Err(error) => apply_activation_failure(&ui, model, error),
                    }
                });
            });
        if let Err(error) = spawn {
            apply_activation_failure(&ui, model, error.to_string());
        }
    });
}
