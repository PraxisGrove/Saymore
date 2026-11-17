use super::{CHUNK_SAMPLES, Chunk, WhisperDecoder, protocol};
use crate::whisper_asr::{AcousticContext, MAX_SEGMENT_SAMPLES, append_segment};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use template_app::SpeechRecognitionError;

pub(super) fn recognize(
    decoder: Arc<dyn WhisperDecoder>,
    incoming: mpsc::Receiver<Chunk>,
    prompt: &str,
    cancelled: &AtomicBool,
    on_partial: Arc<dyn Fn(String) + Send + Sync>,
) -> Result<String, SpeechRecognitionError> {
    let mut window = Window {
        decoder: decoder.as_ref(),
        prompt,
        cancelled,
        on_partial,
        audio: Vec::new(),
        committed: String::new(),
        draft: String::new(),
        displayed: String::new(),
        decoded_samples: 0,
        decoded_context: AcousticContext::Full,
    };
    let mut finished = false;
    while !finished {
        let chunk = incoming
            .recv()
            .map_err(|_| protocol("stream closed before finish"))?;
        finished = window.receive(chunk)?;
        // Keep every full window, but coalesce obsolete intermediate previews.
        while !finished {
            let Ok(chunk) = incoming.try_recv() else {
                break;
            };
            finished = window.receive(chunk)?;
        }
        if !finished && window.audio.len() >= CHUNK_SAMPLES {
            window.decode(AcousticContext::Preview)?;
        }
    }
    window.decode(AcousticContext::Full)?;
    let mut text = window.committed;
    append_segment(&mut text, &window.draft);
    if text.is_empty() {
        return Err(protocol("returned no speech"));
    }
    Ok(text)
}

struct Window<'a> {
    decoder: &'a dyn WhisperDecoder,
    prompt: &'a str,
    cancelled: &'a AtomicBool,
    on_partial: Arc<dyn Fn(String) + Send + Sync>,
    audio: Vec<i16>,
    committed: String,
    draft: String,
    displayed: String,
    decoded_samples: usize,
    decoded_context: AcousticContext,
}

impl Window<'_> {
    fn receive(&mut self, chunk: Chunk) -> Result<bool, SpeechRecognitionError> {
        let (samples, finished) = match chunk {
            Chunk::Audio(samples) => (samples, false),
            Chunk::Finish(samples) => (samples, true),
        };
        self.feed(&samples)?;
        Ok(finished)
    }
    fn feed(&mut self, mut samples: &[i16]) -> Result<(), SpeechRecognitionError> {
        self.check_cancelled()?;
        while !samples.is_empty() {
            let count = (MAX_SEGMENT_SAMPLES - self.audio.len()).min(samples.len());
            self.audio.extend_from_slice(&samples[..count]);
            samples = &samples[count..];
            if self.audio.len() == MAX_SEGMENT_SAMPLES {
                self.decode(AcousticContext::Full)?;
                append_segment(&mut self.committed, &self.draft);
                self.audio.clear();
                self.draft.clear();
                self.decoded_samples = 0;
            }
        }
        Ok(())
    }

    fn decode(&mut self, acoustic: AcousticContext) -> Result<(), SpeechRecognitionError> {
        self.check_cancelled()?;
        if self.audio.is_empty()
            || (self.audio.len() == self.decoded_samples
                && (self.decoded_context == AcousticContext::Full
                    || acoustic == AcousticContext::Preview))
        {
            return Ok(());
        }
        let started = std::time::Instant::now();
        let draft = match acoustic {
            AcousticContext::Full => self.decoder.decode(&self.audio, self.prompt)?,
            AcousticContext::Preview => self.decoder.preview(&self.audio, self.prompt)?,
        };
        tracing::debug!(target: "saymore::diagnostics", event = "asr.whisper.streaming_step",
            audio_samples = self.audio.len(), preview = (acoustic == AcousticContext::Preview),
            elapsed_ms = started.elapsed().as_millis());
        self.check_cancelled()?;
        self.decoded_samples = self.audio.len();
        self.decoded_context = acoustic;
        self.draft = draft;
        let mut text = self.committed.clone();
        append_segment(&mut text, &self.draft);
        if text != self.displayed {
            self.displayed = text.clone();
            (self.on_partial)(text);
        }
        Ok(())
    }

    fn check_cancelled(&self) -> Result<(), SpeechRecognitionError> {
        if self.cancelled.load(Ordering::Acquire) {
            Err(protocol("stream cancelled"))
        } else {
            Ok(())
        }
    }
}
