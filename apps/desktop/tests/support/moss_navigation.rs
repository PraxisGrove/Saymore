use super::{
    AppPage, AppWindow, click,
    ui::{AsrProviderCardKind, AsrProviderCardSpec, LocalModelDownloadState},
};
use slint::{ModelRc, VecModel};
use std::{cell::RefCell, rc::Rc};

pub fn check_moss_actions(ui: &AppWindow) {
    click(ui, 735.0, 150.0);
    ui.set_current_page(AppPage::Models);
    ui.set_model_tab(0);
    ui.set_asr_provider_cards(ModelRc::new(VecModel::from(vec![AsrProviderCardSpec {
        kind: AsrProviderCardKind::Moss,
        available: true,
    }])));
    ui.set_moss_download_supported(true);
    ui.set_moss_adapter_ready(true);
    let events = Rc::new(RefCell::new(Vec::new()));
    let downloads = Rc::clone(&events);
    ui.on_request_local_model_download(move |kind| downloads.borrow_mut().push(("download", kind)));
    let selections = Rc::clone(&events);
    ui.on_request_local_model_selection(move |kind| selections.borrow_mut().push(("select", kind)));
    click(ui, 350.0, 200.0);
    click(ui, 680.0, 350.0);
    ui.set_moss_download_state(LocalModelDownloadState::Downloaded);
    click(ui, 650.0, 150.0);
    assert_eq!(
        *events.borrow(),
        vec![
            ("download", AsrProviderCardKind::Moss),
            ("select", AsrProviderCardKind::Moss)
        ]
    );
}
