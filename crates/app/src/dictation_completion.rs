use std::fmt;

use uuid::Uuid;

use crate::{PcmRecording, RecordingError, SpeechRecognitionError, StreamingRecognitionSession};

/// Stable identity shared by every fact produced for one dictation session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DictationSessionId(Uuid);

impl DictationSessionId {
    pub fn generate() -> Self {
        Self(Uuid::new_v4())
    }
}

impl fmt::Display for DictationSessionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Owns one active recognition session after a caller hands it to completion.
///
/// Callers push ordered audio while recording, then transfer this value so the
/// receiver must consume it through exactly one `finish` or `cancel` call.
pub struct OwnedRecognition {
    session: Box<dyn StreamingRecognitionSession>,
    stream_error: Option<SpeechRecognitionError>,
}

impl OwnedRecognition {
    pub fn new(session: Box<dyn StreamingRecognitionSession>) -> Self {
        Self {
            session,
            stream_error: None,
        }
    }

    pub fn push_audio(&mut self, samples: Vec<i16>) -> Result<(), SpeechRecognitionError> {
        let result = self.session.push_audio(samples);
        if let Err(error) = &result
            && self.stream_error.is_none()
        {
            self.stream_error = Some(error.clone());
        }
        result
    }

    pub fn finish(self) -> Result<String, SpeechRecognitionError> {
        match (self.stream_error, self.session.finish()) {
            (_, Err(provider_error)) => Err(provider_error),
            (Some(stream_error), Ok(_)) => Err(stream_error),
            (None, Ok(transcript)) => Ok(transcript),
        }
    }

    pub fn cancel(self) {
        self.session.cancel();
    }
}

/// Transfers a completed capture attempt into dictation completion.
pub enum DictationHandoff {
    Captured {
        id: DictationSessionId,
        recording: PcmRecording,
        recognition: OwnedRecognition,
    },
    Restored {
        id: DictationSessionId,
        recording: PcmRecording,
    },
    CaptureFailed {
        id: DictationSessionId,
        error: RecordingError,
        recognition: Option<OwnedRecognition>,
    },
}

impl DictationHandoff {
    pub fn id(&self) -> DictationSessionId {
        match self {
            Self::Captured { id, .. }
            | Self::Restored { id, .. }
            | Self::CaptureFailed { id, .. } => *id,
        }
    }
}
