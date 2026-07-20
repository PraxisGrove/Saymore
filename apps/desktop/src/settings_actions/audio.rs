use super::*;

pub(super) fn set_feedback_sounds_enabled(
    ui: slint::Weak<AppWindow>,
    settings: LocalSettingsHandle,
    feedback_sounds_enabled: Arc<AtomicBool>,
    enabled: bool,
) {
    let Some(window) = ui.upgrade() else {
        return;
    };
    let previous = feedback_sounds_enabled.load(Ordering::Acquire);
    window.set_feedback_sounds_status(SharedString::new());
    let failure_ui = ui.clone();
    let committed_feedback_sounds = Arc::clone(&feedback_sounds_enabled);
    let result = settings.submit(
        LocalSettingsChange::SetFeedbackSounds(enabled),
        move |result| {
            if let Some(window) = ui.upgrade() {
                match result {
                    Ok(_) => {
                        committed_feedback_sounds.store(enabled, Ordering::Release);
                        window.set_feedback_sounds_enabled(enabled);
                        window.set_feedback_sounds_status(SharedString::new());
                    }
                    Err(error) => {
                        tracing::warn!(event = "settings.feedback_save_failed", reason = %error);
                        committed_feedback_sounds.store(previous, Ordering::Release);
                        window.set_feedback_sounds_enabled(previous);
                        window.set_feedback_sounds_status(
                            window.global::<Translations>().get_settings_save_failed(),
                        );
                    }
                }
            }
        },
    );
    if let Err(error) = result
        && let Some(window) = failure_ui.upgrade()
    {
        tracing::warn!(event = "settings.feedback_submit_failed", reason = %error);
        feedback_sounds_enabled.store(previous, Ordering::Release);
        window.set_feedback_sounds_enabled(previous);
        window
            .set_feedback_sounds_status(window.global::<Translations>().get_settings_save_failed());
    }
}

pub(super) fn set_mute_system_audio_enabled(
    ui: slint::Weak<AppWindow>,
    settings: LocalSettingsHandle,
    mute_system_audio_enabled: Arc<AtomicBool>,
    enabled: bool,
) {
    let Some(window) = ui.upgrade() else {
        return;
    };
    let previous = mute_system_audio_enabled.load(Ordering::Acquire);
    window.set_mute_system_audio_status(SharedString::new());
    let failure_ui = ui.clone();
    let committed_state = Arc::clone(&mute_system_audio_enabled);
    let result = settings.submit(
        LocalSettingsChange::SetMuteSystemAudio(enabled),
        move |result| {
            if let Some(window) = ui.upgrade() {
                match result {
                    Ok(_) => {
                        committed_state.store(enabled, Ordering::Release);
                        window.set_mute_system_audio_enabled(enabled);
                        window.set_mute_system_audio_status(SharedString::new());
                    }
                    Err(error) => {
                        tracing::warn!(event = "settings.system_audio_mute_save_failed", reason = %error);
                        committed_state.store(previous, Ordering::Release);
                        window.set_mute_system_audio_enabled(previous);
                        window.set_mute_system_audio_status(
                            window.global::<Translations>().get_settings_save_failed(),
                        );
                    }
                }
            }
        },
    );
    if let Err(error) = result
        && let Some(window) = failure_ui.upgrade()
    {
        tracing::warn!(event = "settings.system_audio_mute_submit_failed", reason = %error);
        mute_system_audio_enabled.store(previous, Ordering::Release);
        window.set_mute_system_audio_enabled(previous);
        window.set_mute_system_audio_status(
            window.global::<Translations>().get_settings_save_failed(),
        );
    }
}
