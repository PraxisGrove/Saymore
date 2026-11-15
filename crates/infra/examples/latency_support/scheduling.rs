//! Benchmark scheduling policy, independent of when the caller submits PCM.
//! FinishOnly buffers at the session boundary; it never changes the model graph.
use std::{io, sync::Mutex};
use template_app::{SpeechRecognitionError, StreamingRecognitionSession};

pub struct ScheduledSession {
    inner: Box<dyn StreamingRecognitionSession>,
    pending: Option<Mutex<Vec<i16>>>,
}

impl ScheduledSession {
    pub fn new(inner: Box<dyn StreamingRecognitionSession>, mode: &str) -> Result<Self, io::Error> {
        let pending = match mode {
            "during-capture" => None,
            "finish-only" => Some(Mutex::new(Vec::new())),
            _ => return Err(io::Error::other("invalid scheduling mode")),
        };
        Ok(Self { inner, pending })
    }

    pub fn push_audio(&self, samples: Vec<i16>) -> Result<(), SpeechRecognitionError> {
        match &self.pending {
            Some(pending) => {
                pending.lock().map_err(lock_error)?.extend(samples);
                Ok(())
            }
            None => self.inner.push_audio(samples),
        }
    }

    pub fn finish(self) -> Result<String, SpeechRecognitionError> {
        if let Some(pending) = self.pending {
            self.inner
                .push_audio(pending.into_inner().map_err(lock_error)?)?;
        }
        self.inner.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    struct Observed(Arc<Mutex<Vec<i16>>>);
    impl StreamingRecognitionSession for Observed {
        fn push_audio(&self, samples: Vec<i16>) -> Result<(), SpeechRecognitionError> {
            self.0.lock().map_err(lock_error)?.extend(samples);
            Ok(())
        }
        fn finish(self: Box<Self>) -> Result<String, SpeechRecognitionError> {
            Ok("done".into())
        }
        fn cancel(self: Box<Self>) {}
    }

    #[test]
    fn finish_only_defers_inference_but_preserves_audio_order()
    -> Result<(), Box<dyn std::error::Error>> {
        for mode in ["finish-only", "during-capture"] {
            let observed = Arc::new(Mutex::new(Vec::new()));
            let session = ScheduledSession::new(Box::new(Observed(observed.clone())), mode)?;
            session.push_audio(vec![1, 2])?;
            session.push_audio(vec![3])?;
            assert_eq!(
                observed.lock().map_err(lock_error)?.len(),
                if mode == "finish-only" { 0 } else { 3 }
            );
            assert_eq!(session.finish()?, "done");
            assert_eq!(*observed.lock().map_err(lock_error)?, vec![1, 2, 3]);
        }
        Ok(())
    }
}

fn lock_error<T>(_: std::sync::PoisonError<T>) -> SpeechRecognitionError {
    SpeechRecognitionError::Protocol("benchmark session lock poisoned".into())
}
