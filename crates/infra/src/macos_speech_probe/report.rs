use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct MacOsSpeechProbeReport {
    pub schema_version: u32,
    pub host: ProbeHost,
    pub authorization_before: String,
    pub authorization_after: String,
    pub locales: Vec<LocaleCapability>,
    pub standard_system_managed: RecognitionProbe,
    pub second_system_managed_session: RecognitionProbe,
    pub standard_on_device: RecognitionProbe,
    pub cancellation: CancellationProbe,
    pub long_audio_65_seconds: RecognitionProbe,
    pub integration: IntegrationFacts,
}

#[derive(Debug, Serialize)]
pub struct ProbeHost {
    pub macos_version: String,
    pub architecture: String,
}

#[derive(Debug, Serialize)]
pub struct LocaleCapability {
    pub requested_locale: String,
    pub recognizer_created: bool,
    pub resolved_locale: Option<String>,
    pub currently_available: bool,
    pub supports_on_device_recognition: bool,
}

#[derive(Debug, Serialize)]
pub struct RecognitionProbe {
    pub attempted: bool,
    pub mode: String,
    pub locale: String,
    pub input_audio_seconds: f64,
    pub submitted_audio_seconds: f64,
    pub realtime_paced: bool,
    pub elapsed_ms: u128,
    pub partial_result_count: usize,
    pub final_text: Option<String>,
    pub error: Option<AppleSpeechError>,
}

#[derive(Debug, Serialize)]
pub struct CancellationProbe {
    pub attempted: bool,
    pub task_reported_cancelled: bool,
    pub callback_received_after_cancel: bool,
    pub final_result_received_after_cancel: bool,
    pub callback_error: Option<AppleSpeechError>,
}

#[derive(Debug, Serialize)]
pub struct IntegrationFacts {
    pub requires_saymore_api_key: bool,
    pub requires_saymore_model_download: bool,
    pub apple_service_may_use_network: bool,
    pub apple_service_documents_throttling: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppleSpeechError {
    pub domain: String,
    pub code: isize,
    pub message: String,
}
