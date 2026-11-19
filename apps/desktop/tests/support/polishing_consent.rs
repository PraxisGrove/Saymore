use crate::controls::{AccessibleRole, bounds, click_at};
use crate::ui::{AppPage, AppWindow, LlmProviderConfigurationState, PolishingStrength};
use slint::{ComponentHandle, Timer, platform::Key, platform::WindowEvent};
use std::{cell::RefCell, rc::Rc, time::Duration};

pub fn check_slider_while_confirming(window: AppWindow) {
    window.set_model_tab(1);
    window.set_current_page(AppPage::Models);
    window.set_llm_provider_configured(LlmProviderConfigurationState {
        sensenova: true,
        ..Default::default()
    });
    let requests = Rc::new(RefCell::new(Vec::new()));
    let captured = requests.clone();
    let weak = window.as_weak();
    window.on_request_llm_enable(move |strength| {
        captured.borrow_mut().push(strength);
        if let Some(window) = weak.upgrade() {
            window.set_llm_confirmation_visible(true);
        }
    });

    Timer::single_shot(Duration::ZERO, move || {
        let slider = bounds(&window, AccessibleRole::Slider, "Polishing strength, Off");
        click_at(&window, slider.min_x() + 2.0, slider.center().y);
        press_key(&window, Key::RightArrow);
        assert!(window.get_llm_confirmation_visible());
        assert_eq!(*requests.borrow(), [PolishingStrength::Light]);

        Timer::single_shot(Duration::ZERO, move || {
            press_key(&window, Key::End);
            assert_eq!(*requests.borrow(), [PolishingStrength::Light]);
            let _ = slint::quit_event_loop();
        });
    });
}

fn press_key(window: &AppWindow, key: Key) {
    let text = slint::SharedString::from(key);
    for event in [
        WindowEvent::KeyPressed { text: text.clone() },
        WindowEvent::KeyReleased { text },
    ] {
        assert!(window.window().try_dispatch_event(event).is_ok());
    }
}
