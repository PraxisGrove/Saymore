use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
};
use template_app::{SpeechRecognitionError, StreamingRecognitionSession};

#[cfg(test)]
mod tests;

const MAX_RECORDING_SAMPLES: usize = 30 * 60 * 16_000;

/// Existing model window boundaries, including any overlap needed by its decoder.
pub(crate) struct Chunking {
    pub window_samples: usize,
    pub overlap_samples: usize,
}

/// Moves complete offline-model windows off the capture thread. The decoder and
/// hints are session-owned; only audio not decoded yet is retained. Interim text
/// is delivered by subsequent audio pushes, never by a detached worker.
pub(crate) struct BackgroundRecognition {
    state: Mutex<Option<State>>,
    cancelled: Arc<AtomicBool>,
    on_partial: Arc<dyn Fn(String) + Send + Sync>,
}

struct State {
    chunking: Chunking,
    pending: Vec<i16>,
    tail_has_new_audio: bool,
    received: usize,
    requests: Option<mpsc::Sender<Vec<i16>>>,
    responses: mpsc::Receiver<Result<String, SpeechRecognitionError>>,
    jobs: usize,
    worker: Option<JoinHandle<()>>,
    transcript: String,
    append: fn(&mut String, &str),
    error: Option<SpeechRecognitionError>,
}

impl BackgroundRecognition {
    pub(crate) fn start(
        chunking: Chunking,
        mut decode: impl FnMut(&[i16]) -> Result<String, SpeechRecognitionError> + Send + 'static,
        append: fn(&mut String, &str),
        on_partial: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Result<Box<dyn StreamingRecognitionSession>, SpeechRecognitionError> {
        if chunking.window_samples == 0
            || chunking.overlap_samples >= chunking.window_samples
            || chunking.window_samples > MAX_RECORDING_SAMPLES
        {
            return Err(protocol("invalid recognition window"));
        }
        let (requests, incoming) = mpsc::channel::<Vec<i16>>();
        let (outgoing, responses) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let worker = thread::Builder::new()
            .name("saymore-local-asr-worker".into())
            .spawn(move || {
                for samples in incoming {
                    if worker_cancelled.load(Ordering::Acquire) {
                        break;
                    }
                    let result = decode(&samples);
                    let failed = result.is_err();
                    if worker_cancelled.load(Ordering::Acquire)
                        || outgoing.send(result).is_err()
                        || failed
                    {
                        break;
                    }
                }
            })
            .map_err(|_| transport("could not start recognition worker"))?;
        Ok(Box::new(Self {
            state: Mutex::new(Some(State {
                chunking,
                pending: Vec::new(),
                tail_has_new_audio: false,
                received: 0,
                requests: Some(requests),
                responses,
                jobs: 0,
                worker: Some(worker),
                transcript: String::new(),
                append,
                error: None,
            })),
            cancelled,
            on_partial,
        }))
    }
}

impl StreamingRecognitionSession for BackgroundRecognition {
    fn push_audio(&self, samples: Vec<i16>) -> Result<(), SpeechRecognitionError> {
        let partial = {
            let mut guard = self
                .state
                .lock()
                .map_err(|_| transport("recognition lock was poisoned"))?;
            let state = guard
                .as_mut()
                .ok_or_else(|| transport("recognition already finished"))?;
            if let Some(error) = &state.error {
                return Err(error.clone());
            }
            let previous = state.transcript.clone();
            state.collect_ready()?;
            state.push(&samples)?;
            (state.transcript != previous && !state.transcript.is_empty())
                .then(|| state.transcript.clone())
        };
        if let Some(text) = partial {
            (self.on_partial)(text);
        }
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<String, SpeechRecognitionError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| transport("recognition lock was poisoned"))?
            .take()
            .ok_or_else(|| transport("recognition already finished"))?;
        if let Some(error) = state.error.take() {
            return Err(error);
        }
        if state.received == 0 {
            return Err(protocol("received no audio"));
        }
        if state.tail_has_new_audio {
            let tail = std::mem::take(&mut state.pending);
            state.submit(tail)?;
        }
        // Closing input lets the worker terminate after the last queued window.
        state.requests.take();
        while state.jobs > 0 {
            let result = state
                .responses
                .recv()
                .map_err(|_| transport("recognition worker stopped before completing audio"))?;
            state.apply(result)?;
        }
        if let Some(worker) = state.worker.take() {
            worker
                .join()
                .map_err(|_| transport("recognition worker failed"))?;
        }
        if state.transcript.is_empty() {
            Err(protocol("returned no speech"))
        } else {
            Ok(state.transcript)
        }
    }

    fn cancel(self: Box<Self>) {}
}

impl Drop for BackgroundRecognition {
    fn drop(&mut self) {
        // Native inference already in progress may finish, but queued windows
        // will not be decoded and a cancelled session cannot emit callbacks.
        self.cancelled.store(true, Ordering::Release);
    }
}

impl State {
    fn push(&mut self, samples: &[i16]) -> Result<(), SpeechRecognitionError> {
        if self.received.saturating_add(samples.len()) > MAX_RECORDING_SAMPLES {
            let error = protocol("recording exceeds the 30-minute limit");
            self.error = Some(error.clone());
            return Err(error);
        }
        self.received += samples.len();
        let mut remaining = samples;
        while !remaining.is_empty() {
            let count = (self.chunking.window_samples - self.pending.len()).min(remaining.len());
            self.pending.extend_from_slice(&remaining[..count]);
            remaining = &remaining[count..];
            self.tail_has_new_audio = true;
            if self.pending.len() == self.chunking.window_samples {
                let overlap = self.pending
                    [self.chunking.window_samples - self.chunking.overlap_samples..]
                    .to_vec();
                let window = std::mem::replace(&mut self.pending, overlap);
                self.submit(window)?;
                self.tail_has_new_audio = false;
            }
        }
        Ok(())
    }

    fn submit(&mut self, samples: Vec<i16>) -> Result<(), SpeechRecognitionError> {
        let sent = self
            .requests
            .as_ref()
            .ok_or_else(|| transport("recognition input is closed"))?
            .send(samples);
        if sent.is_err() {
            // A decoder error can close input before finish submits its tail.
            // Prefer that original error over a secondary channel failure.
            self.collect_ready()?;
            let error = transport("recognition worker is unavailable");
            self.error = Some(error.clone());
            return Err(error);
        }
        self.jobs += 1;
        Ok(())
    }

    fn collect_ready(&mut self) -> Result<(), SpeechRecognitionError> {
        while self.jobs > 0 {
            match self.responses.try_recv() {
                Ok(result) => self.apply(result)?,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    let error = transport("recognition worker stopped before completing audio");
                    self.error = Some(error.clone());
                    return Err(error);
                }
            }
        }
        Ok(())
    }

    fn apply(
        &mut self,
        result: Result<String, SpeechRecognitionError>,
    ) -> Result<(), SpeechRecognitionError> {
        self.jobs -= 1;
        match result {
            Ok(segment) => {
                (self.append)(&mut self.transcript, &segment);
                Ok(())
            }
            Err(error) => {
                self.error = Some(error.clone());
                Err(error)
            }
        }
    }
}

fn protocol(message: &str) -> SpeechRecognitionError {
    SpeechRecognitionError::Protocol(format!("Local ASR {message}"))
}
fn transport(message: &str) -> SpeechRecognitionError {
    SpeechRecognitionError::Transport(format!("Local ASR {message}"))
}
