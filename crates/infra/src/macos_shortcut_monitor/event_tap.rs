use super::*;

pub(super) fn start(
    is_recording: Arc<dyn Fn() -> bool + Send + Sync>,
    shortcuts_enabled: Arc<dyn Fn() -> bool + Send + Sync>,
    controller: MacOsShortcutController,
    on_action: impl Fn(DictationShortcutAction) + Send + 'static,
    on_permission_required: impl Fn() + Send + 'static,
) {
    let (sender, receiver) = channel();
    thread::spawn(move || {
        for action in receiver {
            on_action(action);
        }
    });
    thread::spawn(move || {
        #[cfg(debug_assertions)]
        eprintln!("saymore_fn_trace phase=monitor-thread-started");
        #[cfg(debug_assertions)]
        let mut trust_logged = false;
        let mut detector = super::untrusted_poll::UntrustedShortcutDetector::default();
        loop {
            // SAFETY: AXIsProcessTrusted has no preconditions and only reads TCC state.
            let trusted = unsafe { AXIsProcessTrusted() };
            #[cfg(debug_assertions)]
            if !trust_logged {
                eprintln!("saymore_fn_trace phase=accessibility-check trusted={trusted}");
                trust_logged = true;
            }
            if trusted
                && run_event_tap(
                    sender.clone(),
                    Arc::clone(&is_recording),
                    Arc::clone(&shortcuts_enabled),
                    controller.clone(),
                )
                .is_ok()
            {
                return;
            }
            if !trusted {
                if poll_untrusted_shortcuts(&controller, &mut detector) && shortcuts_enabled() {
                    on_permission_required();
                }
            } else {
                thread::sleep(PERMISSION_RETRY_INTERVAL);
            }
        }
    });
}

fn poll_untrusted_shortcuts(
    controller: &MacOsShortcutController,
    detector: &mut super::untrusted_poll::UntrustedShortcutDetector,
) -> bool {
    const POLL_INTERVAL: Duration = Duration::from_millis(20);
    let attempts = PERMISSION_RETRY_INTERVAL.as_millis() / POLL_INTERVAL.as_millis();
    for _ in 0..attempts {
        if let Ok(shortcuts) = controller.current()
            && detector.observe_system(&shortcuts)
        {
            return true;
        }
        thread::sleep(POLL_INTERVAL);
    }
    false
}

fn run_event_tap(
    sender: Sender<DictationShortcutAction>,
    is_recording: Arc<dyn Fn() -> bool + Send + Sync>,
    shortcuts_enabled: Arc<dyn Fn() -> bool + Send + Sync>,
    controller: MacOsShortcutController,
) -> Result<(), ()> {
    let modifier_state = Mutex::new(ModifierState::default());
    let event_tap = CGEventTap::new(
        CGEventTapLocation::HID,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::Default,
        vec![
            CGEventType::FlagsChanged,
            CGEventType::KeyDown,
            CGEventType::KeyUp,
        ],
        move |_proxy, event_type, event: &CGEvent| {
            handle_event(
                event_type,
                event,
                &modifier_state,
                &controller,
                &sender,
                is_recording.as_ref(),
                shortcuts_enabled(),
            )
        },
    );
    #[cfg(debug_assertions)]
    if event_tap.is_err() {
        eprintln!("saymore_fn_trace phase=event-tap-create result=failed");
    }
    let event_tap = event_tap?;
    let source = event_tap.mach_port().create_runloop_source(0)?;
    CFRunLoop::get_current().add_source(&source, unsafe { kCFRunLoopCommonModes });
    event_tap.enable();
    #[cfg(debug_assertions)]
    eprintln!("saymore_fn_trace phase=event-tap-enabled");
    CFRunLoop::run_current();
    Ok(())
}

fn handle_event(
    event_type: CGEventType,
    event: &CGEvent,
    modifier_state: &Mutex<ModifierState>,
    controller: &MacOsShortcutController,
    sender: &Sender<DictationShortcutAction>,
    is_recording: &(dyn Fn() -> bool + Send + Sync),
    shortcuts_enabled: bool,
) -> CallbackResult {
    #[cfg(debug_assertions)]
    if key_code(event) == 63
        || event
            .get_flags()
            .contains(CGEventFlags::CGEventFlagSecondaryFn)
    {
        eprintln!(
            "saymore_fn_trace phase=received event_type={event_type:?} key_code={} flags={:#x}",
            key_code(event),
            event.get_flags().bits()
        );
    }
    match event_type {
        CGEventType::FlagsChanged => {
            handle_modifier_event(event, modifier_state, controller, sender, shortcuts_enabled)
        }
        CGEventType::KeyDown | CGEventType::KeyUp
            if key_code(event) == 63 && controller.reserves_fn(shortcuts_enabled) =>
        {
            CallbackResult::Drop
        }
        CGEventType::KeyDown
            if cancel_shortcut(key_code(event), is_recording()) && !controller.capturing() =>
        {
            mark_active_modifiers_used(modifier_state);
            let _ = sender.send(DictationShortcutAction::Cancel);
            CallbackResult::Drop
        }
        CGEventType::KeyDown if controller.capturing() => {
            mark_active_modifiers_used(modifier_state);
            capture_key_down(event, controller);
            CallbackResult::Drop
        }
        CGEventType::KeyDown => {
            mark_active_modifiers_used(modifier_state);
            handle_shortcut_key_down(event, controller, sender, shortcuts_enabled)
        }
        _ => CallbackResult::Keep,
    }
}

fn capture_key_down(event: &CGEvent, controller: &MacOsShortcutController) {
    if is_repeat(event) {
        return;
    }
    if key_code(event) == ESCAPE_KEY_CODE {
        controller.cancel_capture();
    } else {
        controller.finish_capture(MacOsShortcut::physical(key_code(event), event.get_flags()));
    }
}

pub(super) fn handle_shortcut_key_down(
    event: &CGEvent,
    controller: &MacOsShortcutController,
    sender: &Sender<DictationShortcutAction>,
    shortcuts_enabled: bool,
) -> CallbackResult {
    if !shortcuts_enabled {
        return CallbackResult::Keep;
    }
    let matches = controller
        .current()
        .unwrap_or_default()
        .iter()
        .any(|shortcut| shortcut.matches_key_down(event));
    if !matches {
        return CallbackResult::Keep;
    }
    if !is_repeat(event) {
        let _ = sender.send(DictationShortcutAction::Toggle);
    }
    CallbackResult::Drop
}

#[derive(Default)]
pub(super) struct ModifierState {
    down: HashSet<i64>,
    used_in_chord: HashSet<i64>,
    suppressed: HashSet<i64>,
}

pub(super) fn handle_modifier_event(
    event: &CGEvent,
    modifier_state: &Mutex<ModifierState>,
    controller: &MacOsShortcutController,
    sender: &Sender<DictationShortcutAction>,
    shortcuts_enabled: bool,
) -> CallbackResult {
    let code = key_code(event);
    if !is_modifier_key(code) {
        return CallbackResult::Keep;
    }
    let Ok(mut state) = modifier_state.lock() else {
        return CallbackResult::Keep;
    };
    if modifier_is_down(code, event.get_flags()) {
        if !shortcuts_enabled && !controller.capturing() {
            state.down.remove(&code);
            state.used_in_chord.remove(&code);
            state.suppressed.remove(&code);
            return CallbackResult::Keep;
        }
        state.down.insert(code);
        state.used_in_chord.remove(&code);
        let suppress = code == 63 && controller.reserves_fn(shortcuts_enabled);
        if suppress {
            #[cfg(debug_assertions)]
            eprintln!("saymore_fn_trace phase=modifier-down result=drop");
            state.suppressed.insert(code);
            return CallbackResult::Drop;
        }
        state.suppressed.remove(&code);
        return CallbackResult::Keep;
    }
    let was_down = state.down.remove(&code);
    let used_in_chord = state.used_in_chord.remove(&code);
    let suppressed = state.suppressed.remove(&code);
    drop(state);
    if !was_down {
        return CallbackResult::Keep;
    }
    if used_in_chord {
        #[cfg(debug_assertions)]
        if code == 63 {
            eprintln!(
                "saymore_fn_trace phase=modifier-up chord=true result={}",
                if suppressed { "drop" } else { "keep" }
            );
        }
        return if suppressed {
            CallbackResult::Drop
        } else {
            CallbackResult::Keep
        };
    }
    if controller.capturing() {
        controller.finish_capture(MacOsShortcut::modifier(code));
        return CallbackResult::Drop;
    }
    if !shortcuts_enabled {
        return CallbackResult::Keep;
    }
    let matches_shortcut = controller
        .current()
        .unwrap_or_default()
        .iter()
        .any(|shortcut| shortcut.matches_modifier_release(code));
    if matches_shortcut {
        #[cfg(debug_assertions)]
        if code == 63 {
            eprintln!("saymore_fn_trace phase=modifier-up toggle=true result=drop");
        }
        let _ = sender.send(DictationShortcutAction::Toggle);
        return CallbackResult::Drop;
    }
    CallbackResult::Keep
}

pub(super) fn mark_active_modifiers_used(modifier_state: &Mutex<ModifierState>) {
    if let Ok(mut state) = modifier_state.lock() {
        let down = state.down.iter().copied().collect::<Vec<_>>();
        state.used_in_chord.extend(down);
    }
}

fn is_repeat(event: &CGEvent) -> bool {
    event.get_integer_value_field(EventField::KEYBOARD_EVENT_AUTOREPEAT) != 0
}

fn is_modifier_key(code: i64) -> bool {
    matches!(code, 54..=63)
}

fn modifier_is_down(code: i64, flags: CGEventFlags) -> bool {
    let flag = match code {
        54 | 55 => CGEventFlags::CGEventFlagCommand,
        56 | 60 => CGEventFlags::CGEventFlagShift,
        58 | 61 => CGEventFlags::CGEventFlagAlternate,
        59 | 62 => CGEventFlags::CGEventFlagControl,
        63 => CGEventFlags::CGEventFlagSecondaryFn,
        57 => CGEventFlags::CGEventFlagAlphaShift,
        _ => return false,
    };
    flags.contains(flag)
}
