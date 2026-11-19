use crate::controls::{
    AccessibleRole::{Button, Text},
    bounds, click, visible_bounds, visible_bounds_matching,
};
use crate::ui;

fn check_resource_row(window: &ui::AppWindow, memory_value: &str) {
    let file = bounds(window, Text, "Files");
    let size = bounds(window, Text, "832 MB");
    let memory = bounds(window, Text, "App memory");
    // The footer can also report "Switching". Match the value beside the file size.
    let Some(value) = visible_bounds_matching(window, Text, memory_value, |rect| {
        rect.min_y() < size.max_y() && size.min_y() < rect.max_y()
    }) else {
        eprintln!("Missing memory value {memory_value:?} in the model file row");
        std::process::exit(1);
    };
    assert!(file.max_y() <= size.min_y() && memory.max_y() <= value.min_y());
    assert!(file.min_y() < memory.max_y() && memory.min_y() < file.max_y());
    let disk = file.union(&size);
    let app_memory = memory.union(&value);
    assert!(disk.max_x() <= app_memory.min_x());

    let mode = bounds(window, Text, "Recognition mode");
    let test = bounds(window, Text, "Test");
    let resources = disk.union(&app_memory);
    assert!(mode.max_y() <= resources.min_y() && resources.max_y() <= test.min_y());
}

fn check_footer(window: &ui::AppWindow, status: &str) {
    let delete = bounds(window, Button, "Remove model");
    let test = bounds(window, Text, "Test");
    assert!(test.max_y() <= delete.min_y());
    // The pending state also says "Switching" in the memory row.
    assert!(
        visible_bounds_matching(window, Text, status, |rect| {
            rect.max_x() <= delete.min_x()
                && rect.min_y() <= delete.center().y
                && delete.center().y <= rect.max_y()
        })
        .is_some()
    );
    if window.get_whisper_selected() || window.get_whisper_activation_pending() {
        assert!(visible_bounds(window, Button, "Use model").is_none());
    } else {
        let use_model = bounds(window, Button, "Use model");
        let gap = use_model.min_x() - delete.max_x();
        assert!(gap >= 0.0 && gap < delete.width());
        assert!(use_model.min_y() <= delete.center().y && delete.center().y <= use_model.max_y());
    }
}

pub fn check_installed(window: &ui::AppWindow) {
    check_resource_row(window, "Not enabled");
    check_footer(window, "Installed · Not in use");
    window.set_whisper_test(ui::LocalModelTestState {
        status: ui::LocalModelTestStatus::Running,
        ..Default::default()
    });
    check_delete_unavailable(window);
    window.set_whisper_test(Default::default());
    window.set_whisper_activation_pending(true);
    check_footer(window, "Switching");
    check_delete_unavailable(window);
    window.set_whisper_activation_pending(false);
}

pub fn check_delete_unavailable(window: &ui::AppWindow) {
    click(window, Button, "Remove model");
    assert_eq!(
        window.get_whisper_download_state(),
        ui::LocalModelDownloadState::Downloaded
    );
}

pub fn check_active_memory(window: &ui::AppWindow) {
    check_footer(window, "In use");
    check_resource_row(window, "Unavailable");
    window.set_whisper_runtime_memory_usage("73 MB".into());
    window.set_whisper_runtime_memory_live(true);
    check_resource_row(window, "73 MB");
    window.set_whisper_runtime_memory_usage("86 MB".into());
    check_resource_row(window, "86 MB");
    assert!(visible_bounds(window, Text, "73 MB").is_none());

    window.set_whisper_activation_pending(true);
    check_resource_row(window, "Switching");
    assert!(visible_bounds(window, Text, "86 MB").is_none());
    window.set_whisper_activation_pending(false);
    window.set_whisper_runtime_memory_live(false);
    check_resource_row(window, "Unavailable");
    assert!(visible_bounds(window, Text, "86 MB").is_none());

    window.set_whisper_runtime_memory_live(true);
    window.set_whisper_selected(false);
    check_resource_row(window, "Not enabled");
    assert!(visible_bounds(window, Text, "86 MB").is_none());
}
