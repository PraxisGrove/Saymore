use std::sync::Arc;

use template_app::{
    AudioInputDevice, AudioRecorder, OutputAudioMuteSession, OutputAudioMuter, PcmChunk,
    PcmRecording, RecordingError, RecordingMetrics, RecordingStarted,
};

pub(crate) struct RecordingAudio {
    recorder: Box<dyn AudioRecorder>,
    output_audio_muter: Arc<dyn OutputAudioMuter>,
    mute_session: Option<Box<dyn OutputAudioMuteSession>>,
}

impl RecordingAudio {
    pub(crate) fn new(
        recorder: Box<dyn AudioRecorder>,
        output_audio_muter: Arc<dyn OutputAudioMuter>,
    ) -> Self {
        Self {
            recorder,
            output_audio_muter,
            mute_session: None,
        }
    }

    pub(crate) fn begin_output_mute(&mut self, enabled: bool) {
        if !enabled || self.mute_session.is_some() {
            return;
        }
        match self.output_audio_muter.begin_mute() {
            Ok(session) => {
                self.mute_session = Some(session);
                tracing::info!(
                    target: "saymore::diagnostics",
                    event = "recording.system_audio_muted"
                );
            }
            Err(error) => {
                tracing::warn!(
                    target: "saymore::diagnostics",
                    event = "recording.system_audio_mute_failed",
                    reason = %error
                );
            }
        }
    }

    fn restore_output_audio(&mut self) {
        if let Some(mut session) = self.mute_session.take() {
            match session.restore() {
                Ok(()) => tracing::info!(
                    target: "saymore::diagnostics",
                    event = "recording.system_audio_restored"
                ),
                Err(error) => tracing::warn!(
                    target: "saymore::diagnostics",
                    event = "recording.system_audio_restore_failed",
                    reason = %error
                ),
            }
        }
    }
}

impl AudioRecorder for RecordingAudio {
    fn input_devices(&self) -> Result<Vec<AudioInputDevice>, RecordingError> {
        let result = self.recorder.input_devices();
        match &result {
            Ok(devices) => tracing::info!(
                target: "saymore::diagnostics",
                event = "microphone.devices_listed",
                device_count = devices.len()
            ),
            Err(error) => tracing::warn!(
                target: "saymore::diagnostics",
                event = "microphone.device_list_failed",
                reason = %error
            ),
        }
        result
    }

    fn set_preferred_input_device_id(&mut self, id: Option<String>) {
        self.recorder.set_preferred_input_device_id(id);
        tracing::info!(
            target: "saymore::diagnostics",
            event = "microphone.preferred_device_applied"
        );
    }

    fn prepare(&mut self) -> Result<(), RecordingError> {
        let result = self.recorder.prepare();
        if let Err(error) = &result {
            tracing::warn!(
                target: "saymore::diagnostics",
                event = "recording.audio_prepare_failed",
                reason = %error
            );
        }
        result
    }

    fn is_recording(&self) -> bool {
        self.recorder.is_recording()
    }

    fn start(
        &mut self,
        on_metrics: Arc<dyn Fn(RecordingMetrics) + Send + Sync>,
        on_audio_chunk: Arc<dyn Fn(PcmChunk) + Send + Sync>,
    ) -> Result<RecordingStarted, RecordingError> {
        self.restore_output_audio();
        let result = self.recorder.start(on_metrics, on_audio_chunk);
        if let Err(error) = &result {
            tracing::warn!(
                target: "saymore::diagnostics",
                event = "recording.audio_start_failed",
                reason = %error
            );
        }
        result
    }

    fn stop(&mut self) -> Result<PcmRecording, RecordingError> {
        let result = self.recorder.stop();
        self.restore_output_audio();
        match &result {
            Ok(_) => tracing::info!(
                target: "saymore::diagnostics",
                event = "recording.audio_stopped"
            ),
            Err(error) => tracing::warn!(
                target: "saymore::diagnostics",
                event = "recording.audio_stop_failed",
                reason = %error
            ),
        }
        result
    }

    fn cancel(&mut self) -> Result<(), RecordingError> {
        let result = self.recorder.cancel();
        self.restore_output_audio();
        match &result {
            Ok(()) => tracing::info!(
                target: "saymore::diagnostics",
                event = "recording.audio_cancelled"
            ),
            Err(error) => tracing::warn!(
                target: "saymore::diagnostics",
                event = "recording.audio_cancel_failed",
                reason = %error
            ),
        }
        result
    }
}

impl Drop for RecordingAudio {
    fn drop(&mut self) {
        self.restore_output_audio();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use template_app::SystemAudioMuteError;

    use super::*;

    #[derive(Default)]
    struct FakeRecorder {
        fail_stop: bool,
    }

    impl AudioRecorder for FakeRecorder {
        fn input_devices(&self) -> Result<Vec<AudioInputDevice>, RecordingError> {
            Ok(Vec::new())
        }

        fn set_preferred_input_device_id(&mut self, _id: Option<String>) {}

        fn prepare(&mut self) -> Result<(), RecordingError> {
            Ok(())
        }

        fn is_recording(&self) -> bool {
            true
        }

        fn start(
            &mut self,
            _on_metrics: Arc<dyn Fn(RecordingMetrics) + Send + Sync>,
            _on_audio_chunk: Arc<dyn Fn(PcmChunk) + Send + Sync>,
        ) -> Result<RecordingStarted, RecordingError> {
            Ok(RecordingStarted {
                input_device_name: "test microphone".to_owned(),
                used_system_fallback: false,
            })
        }

        fn stop(&mut self) -> Result<PcmRecording, RecordingError> {
            if self.fail_stop {
                return Err(RecordingError::Capture("injected stop failure".to_owned()));
            }
            Ok(PcmRecording {
                samples: Vec::new(),
                sample_rate: 16_000,
                channels: 1,
                duration_ms: 0,
            })
        }

        fn cancel(&mut self) -> Result<(), RecordingError> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeMuter {
        begins: AtomicUsize,
        restores: Arc<AtomicUsize>,
    }

    impl OutputAudioMuter for FakeMuter {
        fn begin_mute(&self) -> Result<Box<dyn OutputAudioMuteSession>, SystemAudioMuteError> {
            self.begins.fetch_add(1, Ordering::Relaxed);
            Ok(Box::new(FakeMuteSession {
                restores: Arc::clone(&self.restores),
                restored: false,
            }))
        }
    }

    struct FakeMuteSession {
        restores: Arc<AtomicUsize>,
        restored: bool,
    }

    impl OutputAudioMuteSession for FakeMuteSession {
        fn restore(&mut self) -> Result<(), SystemAudioMuteError> {
            if !self.restored {
                self.restored = true;
                self.restores.fetch_add(1, Ordering::Relaxed);
            }
            Ok(())
        }
    }

    fn coordinator(muter: Arc<FakeMuter>) -> RecordingAudio {
        RecordingAudio::new(Box::new(FakeRecorder::default()), muter)
    }

    #[test]
    fn disabled_preference_does_not_start_a_mute_session() {
        let muter = Arc::new(FakeMuter::default());
        let mut audio = coordinator(Arc::clone(&muter));

        audio.begin_output_mute(false);

        assert_eq!(0, muter.begins.load(Ordering::Relaxed));
    }

    #[test]
    fn stopping_recording_restores_the_active_mute_session() {
        let muter = Arc::new(FakeMuter::default());
        let mut audio = coordinator(Arc::clone(&muter));
        audio.begin_output_mute(true);

        assert!(audio.stop().is_ok());
        assert_eq!(1, muter.restores.load(Ordering::Relaxed));
    }

    #[test]
    fn dropping_recording_audio_restores_the_active_mute_session() {
        let muter = Arc::new(FakeMuter::default());
        {
            let mut audio = coordinator(Arc::clone(&muter));
            audio.begin_output_mute(true);
        }

        assert_eq!(1, muter.restores.load(Ordering::Relaxed));
    }

    #[test]
    fn failed_stop_still_restores_the_active_mute_session() {
        let muter = Arc::new(FakeMuter::default());
        let output_audio_muter: Arc<dyn OutputAudioMuter> = muter.clone();
        let mut audio = RecordingAudio::new(
            Box::new(FakeRecorder { fail_stop: true }),
            output_audio_muter,
        );
        audio.begin_output_mute(true);

        assert!(matches!(audio.stop(), Err(RecordingError::Capture(_))));
        assert_eq!(1, muter.restores.load(Ordering::Relaxed));
    }

    #[test]
    fn cancellation_restores_the_active_mute_session() {
        let muter = Arc::new(FakeMuter::default());
        let mut audio = coordinator(Arc::clone(&muter));
        audio.begin_output_mute(true);

        assert!(audio.cancel().is_ok());
        assert_eq!(1, muter.restores.load(Ordering::Relaxed));
    }
}
