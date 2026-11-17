use std::{env, io, path::Path, sync::Arc};
use template_app::{
    SpeechRecognitionError, SpeechRecognitionHints, StreamingRecognitionSession,
    StreamingSpeechRecognizer, WhisperRecognitionMode,
};
use template_infra::WhisperSpeechRecognizer;

pub(crate) struct WhisperReplay {
    model: WhisperSpeechRecognizer,
    mode: WhisperRecognitionMode,
}
impl WhisperReplay {
    pub fn load(directory: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let mode = match env::var("SAYMORE_BENCH_WHISPER_MODE")
            .as_deref()
            .unwrap_or("non_streaming")
        {
            "non_streaming" => WhisperRecognitionMode::NonStreaming,
            "streaming" => WhisperRecognitionMode::Streaming,
            _ => return Err(io::Error::other("invalid Whisper benchmark mode").into()),
        };
        Ok(Self {
            model: WhisperSpeechRecognizer::load(directory)?,
            mode,
        })
    }
}
impl StreamingSpeechRecognizer for WhisperReplay {
    fn start(
        &self,
        hints: SpeechRecognitionHints,
        partial: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Result<Box<dyn StreamingRecognitionSession>, SpeechRecognitionError> {
        self.model.start_with_mode(self.mode, hints, partial)
    }
}
