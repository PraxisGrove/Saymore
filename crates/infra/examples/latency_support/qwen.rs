use std::{env, io, path::Path, sync::Arc};
use template_app::{
    QwenRecognitionMode, SpeechRecognitionError, SpeechRecognitionHints,
    StreamingRecognitionSession, StreamingSpeechRecognizer,
};
use template_infra::Qwen3AsrSpeechRecognizer;

pub(crate) struct QwenReplay {
    model: Qwen3AsrSpeechRecognizer,
    mode: QwenRecognitionMode,
}
impl QwenReplay {
    pub fn load(directory: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let mode = match env::var("SAYMORE_BENCH_QWEN_MODE")
            .as_deref()
            .unwrap_or("non_streaming")
        {
            "non_streaming" => QwenRecognitionMode::NonStreaming,
            "streaming" => QwenRecognitionMode::Streaming,
            _ => return Err(io::Error::other("invalid Qwen benchmark mode").into()),
        };
        Ok(Self {
            model: Qwen3AsrSpeechRecognizer::load(directory)?,
            mode,
        })
    }
}
impl StreamingSpeechRecognizer for QwenReplay {
    fn start(
        &self,
        hints: SpeechRecognitionHints,
        partial: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Result<Box<dyn StreamingRecognitionSession>, SpeechRecognitionError> {
        self.model.start_with_mode(self.mode, hints, partial)
    }
}
