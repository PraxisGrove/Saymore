#![allow(clippy::panic_in_result_fn)]

use super::*;
use std::{
    error::Error,
    time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_secs(3);

fn append(text: &mut String, segment: &str) {
    text.push_str(segment);
}

#[test]
fn complete_windows_start_before_finish_without_blocking_capture() -> Result<(), Box<dyn Error>> {
    let (entered, started) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&seen);
    let session = BackgroundRecognition::start(
        Chunking {
            window_samples: 4,
            overlap_samples: 0,
        },
        move |audio| {
            captured
                .lock()
                .map_err(|_| protocol("test lock"))?
                .push(audio.to_vec());
            if audio[0] == 0 {
                let _ = entered.send(());
                resume
                    .recv_timeout(WAIT)
                    .map_err(|_| protocol("test release timed out"))?;
            }
            Ok("x".into())
        },
        append,
        Arc::new(|_| {}),
    )?;
    session.push_audio((0..10).collect())?;
    started.recv_timeout(WAIT)?;
    // Capture already returned although inference is blocked until released.
    release.send(())?;
    assert_eq!("xxx", session.finish()?);
    assert_eq!(
        *seen.lock().map_err(|_| protocol("test lock"))?,
        vec![vec![0, 1, 2, 3], vec![4, 5, 6, 7], vec![8, 9]]
    );
    Ok(())
}

#[test]
fn overlap_and_exact_boundaries_match_final_only_segmentation() -> Result<(), Box<dyn Error>> {
    for (length, expected) in [
        (4, vec![vec![0, 1, 2, 3]]),
        (5, vec![vec![0, 1, 2, 3], vec![3, 4]]),
        (7, vec![vec![0, 1, 2, 3], vec![3, 4, 5, 6]]),
        (8, vec![vec![0, 1, 2, 3], vec![3, 4, 5, 6], vec![6, 7]]),
    ] {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&seen);
        let session = BackgroundRecognition::start(
            Chunking {
                window_samples: 4,
                overlap_samples: 1,
            },
            move |audio| {
                captured
                    .lock()
                    .map_err(|_| protocol("test lock"))?
                    .push(audio.to_vec());
                Ok("x".into())
            },
            append,
            Arc::new(|_| {}),
        )?;
        for sample in 0..length {
            session.push_audio(vec![sample])?;
        }
        session.finish()?;
        assert_eq!(expected, *seen.lock().map_err(|_| protocol("test lock"))?);
    }
    Ok(())
}

#[test]
fn completed_text_is_reported_during_capture_and_not_duplicated() -> Result<(), Box<dyn Error>> {
    let (partial, observed) = mpsc::channel();
    let session = BackgroundRecognition::start(
        Chunking {
            window_samples: 4,
            overlap_samples: 0,
        },
        |_| Ok("segment".into()),
        append,
        Arc::new(move |text| {
            let _ = partial.send(text);
        }),
    )?;
    session.push_audio(vec![1; 4])?;
    let deadline = Instant::now() + WAIT;
    let text = loop {
        session.push_audio(Vec::new())?;
        if let Ok(text) = observed.try_recv() {
            break text;
        }
        if Instant::now() >= deadline {
            return Err("no partial result before finish".into());
        }
        thread::sleep(Duration::from_millis(1));
    };
    assert_eq!("segment", text);
    session.push_audio(Vec::new())?;
    assert!(observed.try_recv().is_err());
    assert_eq!("segment", session.finish()?);
    Ok(())
}

struct WorkerDone(mpsc::Sender<()>);
impl Drop for WorkerDone {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}

#[test]
fn cancellation_discards_queued_windows_and_never_reports_worker_results()
-> Result<(), Box<dyn Error>> {
    let (entered, started) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let (done, finished) = mpsc::channel();
    let done = WorkerDone(done);
    let (partial, observed) = mpsc::channel();
    let session = BackgroundRecognition::start(
        Chunking {
            window_samples: 4,
            overlap_samples: 0,
        },
        move |_| {
            let _keep_alive = &done;
            let _ = entered.send(());
            resume
                .recv_timeout(WAIT)
                .map_err(|_| protocol("test release timed out"))?;
            Ok("cancelled".into())
        },
        append,
        Arc::new(move |text| {
            let _ = partial.send(text);
        }),
    )?;
    session.push_audio(vec![1; 12])?;
    started.recv_timeout(WAIT)?;
    session.cancel();
    release.send(())?;
    finished.recv_timeout(WAIT)?;
    assert!(started.try_recv().is_err());
    assert!(observed.try_recv().is_err());
    Ok(())
}

#[test]
fn worker_error_is_returned_instead_of_incomplete_text() -> Result<(), SpeechRecognitionError> {
    let expected = protocol("decoder failed");
    let failure = expected.clone();
    let session = BackgroundRecognition::start(
        Chunking {
            window_samples: 4,
            overlap_samples: 0,
        },
        move |_| Err(failure.clone()),
        append,
        Arc::new(|_| {}),
    )?;
    session.push_audio(vec![1; 5])?;
    assert_eq!(Err(expected), session.finish());
    Ok(())
}

#[test]
fn empty_audio_and_invalid_windows_are_rejected() -> Result<(), SpeechRecognitionError> {
    for chunking in [
        Chunking {
            window_samples: 0,
            overlap_samples: 0,
        },
        Chunking {
            window_samples: 4,
            overlap_samples: 4,
        },
    ] {
        assert!(
            BackgroundRecognition::start(chunking, |_| Ok("x".into()), append, Arc::new(|_| {}))
                .is_err()
        );
    }
    let empty = BackgroundRecognition::start(
        Chunking {
            window_samples: 4,
            overlap_samples: 0,
        },
        |_| Ok("x".into()),
        append,
        Arc::new(|_| {}),
    )?;
    assert!(empty.finish().is_err());
    Ok(())
}
