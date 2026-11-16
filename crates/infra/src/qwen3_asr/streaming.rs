//! Official-algorithm port: cumulative audio, two initial unfixed chunks, then
//! a five-token rollback performed by the native tokenizer before continuation.
use super::{Qwen3Decoder, process::protocol};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
};
use template_app::{SpeechRecognitionError, StreamingRecognitionSession};

const CHUNK_SAMPLES: usize = 2 * 16_000;
const MAX_SAMPLES: usize = 20 * 60 * 16_000;
// The 20-minute input limit bounds queued and cumulative audio together.

pub(super) fn start(
    decoder: Arc<dyn Qwen3Decoder>,
    hotwords: String,
    on_partial: Arc<dyn Fn(String) + Send + Sync>,
) -> Result<Box<dyn StreamingRecognitionSession>, SpeechRecognitionError> {
    let (send, incoming) = mpsc::channel();
    let (complete, result) = mpsc::channel();
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_cancelled = Arc::clone(&cancelled);
    let worker = thread::Builder::new()
        .name("saymore-qwen-streaming".into())
        .spawn(move || {
            let outcome = recognize(decoder, incoming, &hotwords, &worker_cancelled, on_partial);
            let _ = complete.send(outcome);
        })
        .map_err(|_| protocol("could not start streaming recognition"))?;
    Ok(Box::new(Session {
        state: Mutex::new(Some(State {
            send: Some(send),
            pending: Vec::new(),
            received: 0,
            result,
            worker: Some(worker),
            error: None,
        })),
        cancelled,
    }))
}

struct Session {
    state: Mutex<Option<State>>,
    cancelled: Arc<AtomicBool>,
}
struct State {
    send: Option<mpsc::Sender<Vec<i16>>>,
    pending: Vec<i16>,
    received: usize,
    result: mpsc::Receiver<Result<String, SpeechRecognitionError>>,
    worker: Option<JoinHandle<()>>,
    error: Option<SpeechRecognitionError>,
}

impl StreamingRecognitionSession for Session {
    fn push_audio(&self, samples: Vec<i16>) -> Result<(), SpeechRecognitionError> {
        let mut guard = self
            .state
            .lock()
            .map_err(|_| protocol("stream lock was poisoned"))?;
        let state = guard
            .as_mut()
            .ok_or_else(|| protocol("stream already finished"))?;
        if let Some(error) = &state.error {
            return Err(error.clone());
        }
        if let Ok(Err(error)) = state.result.try_recv() {
            state.error = Some(error.clone());
            return Err(error);
        }
        let outcome = state.push(&samples);
        if let Err(error) = &outcome {
            self.cancelled.store(true, Ordering::Release);
            state.error = Some(error.clone());
        }
        outcome
    }

    fn finish(self: Box<Self>) -> Result<String, SpeechRecognitionError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| protocol("stream lock was poisoned"))?
            .take()
            .ok_or_else(|| protocol("stream already finished"))?;
        if let Some(error) = state.error.take() {
            return Err(error);
        }
        if state.received == 0 {
            return Err(protocol("received no audio"));
        }
        if let Some(send) = state.send.take() {
            if !state.pending.is_empty() {
                let _ = send.send(std::mem::take(&mut state.pending));
            }
            drop(send);
        }
        let result = state
            .result
            .recv()
            .map_err(|_| protocol("stream stopped before finishing"))?;
        if let Some(worker) = state.worker.take() {
            worker
                .join()
                .map_err(|_| protocol("stream worker failed"))?;
        }
        result
    }

    fn cancel(self: Box<Self>) {}
}

impl Drop for Session {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

impl State {
    fn push(&mut self, samples: &[i16]) -> Result<(), SpeechRecognitionError> {
        if self.received.saturating_add(samples.len()) > MAX_SAMPLES {
            return Err(protocol(
                "streaming recording exceeds the 20-minute model window",
            ));
        }
        self.received += samples.len();
        let mut remaining = samples;
        while !remaining.is_empty() {
            let count = (CHUNK_SAMPLES - self.pending.len()).min(remaining.len());
            self.pending.extend_from_slice(&remaining[..count]);
            remaining = &remaining[count..];
            if self.pending.len() == CHUNK_SAMPLES {
                self.send
                    .as_ref()
                    .ok_or_else(|| protocol("stream input closed"))?
                    .send(std::mem::take(&mut self.pending))
                    .map_err(|_| protocol("stream worker stopped"))?;
            }
        }
        Ok(())
    }
}

fn recognize(
    decoder: Arc<dyn Qwen3Decoder>,
    incoming: mpsc::Receiver<Vec<i16>>,
    hotwords: &str,
    cancelled: &AtomicBool,
    on_partial: Arc<dyn Fn(String) + Send + Sync>,
) -> Result<String, SpeechRecognitionError> {
    let mut audio = Vec::new();
    let mut raw = String::new();
    let mut text = String::new();
    let mut index = 0;
    while let Ok(samples) = incoming.recv() {
        if cancelled.load(Ordering::Acquire) {
            return Err(protocol("stream cancelled"));
        }
        audio.extend(samples);
        // When inference falls behind, coalesce pending steps, never their audio.
        // Keep the first two actual decodes unfixed even on a slow device.
        for pending in incoming.try_iter() {
            audio.extend(pending);
        }
        let prefix = if index < 2 { "" } else { &raw };
        let started = std::time::Instant::now();
        let decoded = decoder.decode_stream(&audio, hotwords, prefix)?;
        tracing::debug!(target: "saymore::diagnostics", event = "asr.qwen.streaming_step",
            step = index, audio_samples = audio.len(), elapsed_ms = started.elapsed().as_millis());
        index += 1;
        raw = decoded.raw;
        let changed = text != decoded.text;
        text = decoded.text;
        if cancelled.load(Ordering::Acquire) {
            return Err(protocol("stream cancelled"));
        }
        if changed {
            on_partial(text.clone());
        }
    }
    if text.trim().is_empty() {
        return Err(protocol("returned no speech"));
    }
    Ok(text)
}

#[cfg(test)]
mod tests;
