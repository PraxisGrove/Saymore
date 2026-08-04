use std::sync::Arc;

use template_app::{SpeechRecognitionError, StreamingRecognitionSession};
use template_infra::OfflinePunctuationRestorer;

pub(super) struct PunctuatedRecognitionSession {
    pub(super) inner: Box<dyn StreamingRecognitionSession>,
    pub(super) punctuation: Arc<OfflinePunctuationRestorer>,
}

impl StreamingRecognitionSession for PunctuatedRecognitionSession {
    fn push_audio(&self, samples: Vec<i16>) -> Result<(), SpeechRecognitionError> {
        self.inner.push_audio(samples)
    }

    fn finish(self: Box<Self>) -> Result<String, SpeechRecognitionError> {
        let raw = self.inner.finish()?;
        Ok(restore_or_raw(&self.punctuation, raw))
    }

    fn cancel(self: Box<Self>) {
        self.inner.cancel();
    }
}

pub(super) fn restore_or_raw(punctuation: &OfflinePunctuationRestorer, raw: String) -> String {
    restore_or_raw_with(raw, |text| punctuation.add_punctuation(text))
}

fn restore_or_raw_with(
    raw: String,
    restore: impl FnOnce(&str) -> Result<String, SpeechRecognitionError>,
) -> String {
    if raw.trim().is_empty() {
        return raw;
    }
    match restore(&raw) {
        Ok(punctuated) => punctuated,
        Err(error) => {
            tracing::warn!(
                target: "saymore::diagnostics",
                event = "punctuation.inference_failed",
                reason = %error
            );
            raw
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    #[test]
    fn punctuation_failure_preserves_the_raw_final() {
        let raw = "今天天气很好".to_owned();

        let restored = restore_or_raw_with(raw.clone(), |_| {
            Err(SpeechRecognitionError::Protocol("failed".to_owned()))
        });

        assert_eq!(raw, restored);
    }

    #[test]
    fn blank_final_skips_punctuation_inference() {
        let called = AtomicBool::new(false);

        let restored = restore_or_raw_with("  ".to_owned(), |_| {
            called.store(true, Ordering::Relaxed);
            Ok("unexpected".to_owned())
        });

        assert_eq!("  ", restored);
        assert!(!called.load(Ordering::Relaxed));
    }
}
