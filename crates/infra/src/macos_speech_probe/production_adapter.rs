use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use template_app::{
    SpeechRecognitionError, SpeechRecognitionHints, StreamingRecognitionSession,
    StreamingSpeechRecognizer,
};

use super::{
    RecognitionProbe, STREAM_CHUNK_SAMPLES, audio_seconds, probe_error, skipped_recognition,
};
use crate::MacOsSpeechRecognizer;

const MODE: &str = "production-adapter-system-managed";

pub(super) fn recognize_with_production_adapter(
    samples: &[i16],
    realtime_paced: bool,
    authorized: bool,
    locale: &str,
) -> RecognitionProbe {
    if !authorized {
        return skipped_recognition(
            samples,
            MODE,
            locale,
            "speech recognition is not authorized",
        );
    }
    let context = AdapterProbeContext {
        samples,
        realtime_paced,
        locale,
        started: Instant::now(),
        partial_count: Arc::new(AtomicUsize::new(0)),
    };
    let recognizer = match MacOsSpeechRecognizer::for_locale(locale) {
        Ok(recognizer) => recognizer,
        Err(error) => return context.report(0, Err(error)),
    };
    let callback_count = Arc::clone(&context.partial_count);
    let session = recognizer.start(
        SpeechRecognitionHints::default(),
        Arc::new(move |_| {
            callback_count.fetch_add(1, Ordering::Relaxed);
        }),
    );
    match session {
        Ok(session) => context.recognize(session),
        Err(error) => context.report(0, Err(error)),
    }
}

struct AdapterProbeContext<'a> {
    samples: &'a [i16],
    realtime_paced: bool,
    locale: &'a str,
    started: Instant,
    partial_count: Arc<AtomicUsize>,
}

impl AdapterProbeContext<'_> {
    fn recognize(&self, session: Box<dyn StreamingRecognitionSession>) -> RecognitionProbe {
        let mut submitted_samples = 0;
        for chunk in self.samples.chunks(STREAM_CHUNK_SAMPLES) {
            if let Err(error) = session.push_audio(chunk.to_vec()) {
                session.cancel();
                return self.report(submitted_samples, Err(error));
            }
            submitted_samples += chunk.len();
            if self.realtime_paced {
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        self.report(submitted_samples, session.finish())
    }

    fn report(
        &self,
        submitted_samples: usize,
        result: Result<String, SpeechRecognitionError>,
    ) -> RecognitionProbe {
        let (final_text, error) = match result {
            Ok(text) => (Some(text), None),
            Err(error) => (
                None,
                Some(probe_error(
                    "SaymoreAppleSpeechAdapter",
                    -4,
                    &error.to_string(),
                )),
            ),
        };
        RecognitionProbe {
            attempted: true,
            mode: MODE.to_owned(),
            locale: self.locale.to_owned(),
            input_audio_seconds: audio_seconds(self.samples.len()),
            submitted_audio_seconds: audio_seconds(submitted_samples),
            realtime_paced: self.realtime_paced,
            elapsed_ms: self.started.elapsed().as_millis(),
            partial_result_count: self.partial_count.load(Ordering::Relaxed),
            final_text,
            error,
        }
    }
}
