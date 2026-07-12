use std::sync::{Arc, Mutex};

use template_app::{
    SettingsStore, SpeechRecognitionError, StreamingRecognitionSession, StreamingSpeechRecognizer,
};
use template_infra::{JsonSettingsStore, VolcengineSpeechRecognizer};

pub struct AsrSessionController {
    settings: Arc<JsonSettingsStore>,
    active: Mutex<Option<Box<dyn StreamingRecognitionSession>>>,
    stream_error: Mutex<Option<SpeechRecognitionError>>,
}

impl AsrSessionController {
    pub fn new(settings: Arc<JsonSettingsStore>) -> Self {
        Self {
            settings,
            active: Mutex::new(None),
            stream_error: Mutex::new(None),
        }
    }

    pub fn start(
        &self,
        on_partial: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Result<(), SpeechRecognitionError> {
        let settings = self
            .settings
            .load()
            .map_err(|error| SpeechRecognitionError::Protocol(error.to_string()))?;
        let provider = settings.asr.volcengine;
        if !provider.enabled {
            return Err(SpeechRecognitionError::NotConfigured);
        }
        let mut active = self
            .active
            .lock()
            .map_err(|_| SpeechRecognitionError::Transport("ASR lock was poisoned".to_owned()))?;
        if active.is_some() {
            return Err(SpeechRecognitionError::Transport(
                "ASR session is already active".to_owned(),
            ));
        }
        let recognizer = VolcengineSpeechRecognizer::new(provider.api_key)?;
        let session = recognizer.start(on_partial)?;
        if let Ok(mut stream_error) = self.stream_error.lock() {
            *stream_error = None;
        }
        *active = Some(session);
        Ok(())
    }

    pub fn push_audio(&self, samples: Vec<i16>) -> Result<(), SpeechRecognitionError> {
        let active = self
            .active
            .lock()
            .map_err(|_| SpeechRecognitionError::Transport("ASR lock was poisoned".to_owned()))?;
        let session = active.as_ref().ok_or_else(|| {
            SpeechRecognitionError::Transport("ASR session is inactive".to_owned())
        })?;
        let result = session.push_audio(samples);
        drop(active);
        if let Err(error) = &result
            && let Ok(mut stream_error) = self.stream_error.lock()
            && stream_error.is_none()
        {
            *stream_error = Some(error.clone());
        }
        result
    }

    pub fn finish(&self) -> Result<String, SpeechRecognitionError> {
        let session = self
            .active
            .lock()
            .map_err(|_| SpeechRecognitionError::Transport("ASR lock was poisoned".to_owned()))?
            .take()
            .ok_or_else(|| {
                SpeechRecognitionError::Transport("ASR session is inactive".to_owned())
            })?;
        let stream_error = self
            .stream_error
            .lock()
            .map_err(|_| SpeechRecognitionError::Transport("ASR lock was poisoned".to_owned()))?
            .take();
        if let Some(error) = stream_error {
            session.cancel();
            return Err(error);
        }
        session.finish()
    }

    pub fn cancel(&self) {
        if let Ok(mut active) = self.active.lock()
            && let Some(session) = active.take()
        {
            session.cancel();
        }
        if let Ok(mut stream_error) = self.stream_error.lock() {
            *stream_error = None;
        }
    }
}

pub fn normalize_transcript(transcript: &str) -> String {
    transcript.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn error_message(error: &SpeechRecognitionError) -> &'static str {
    match error {
        SpeechRecognitionError::NotConfigured => "请先配置火山引擎 API Key",
        SpeechRecognitionError::Authentication => "火山引擎 API Key 无效",
        SpeechRecognitionError::Quota => "火山引擎额度不足或请求受限",
        SpeechRecognitionError::Transport(_) => "无法连接火山引擎",
        SpeechRecognitionError::Protocol(_) => "火山引擎返回了无法解析的结果",
        SpeechRecognitionError::Timeout => "语音识别超时",
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_transcript;

    #[test]
    fn normalizes_surrounding_and_repeated_whitespace() {
        assert_eq!(
            "你好 Saymore。",
            normalize_transcript("  你好   Saymore。\n")
        );
    }
}
