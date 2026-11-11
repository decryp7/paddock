//! The toggle's recorder: one recording at a time, through the [`Capture`] port, into a bounded 16 kHz mono clip.
//!
//! A worker thread per recording drains the hand-over into a `ClipBuilder` (the audio thread only copies). The elapsed time the
//! composer shows is the audio recorded so far, so the recorder needs no clock (ER-18: the engine reports it, the pane keeps
//! none). At the cap the worker closes the device itself; a device failure closes it too and is kept as the recording's
//! outcome.

use super::capture::{Capture, CaptureError, Chunk, Frames, OpenCapture, CHANNEL_CHUNKS};
use pitwall_core::speech::pcm::{Clip, ClipBuilder, Push};
use pitwall_core::speech::MAX_CLIP_SAMPLES;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;

/// Where the recorder is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// Not recording.
    Idle,
    /// Recording; `recorded_ms` of audio so far.
    Recording {
        /// Audio recorded so far, in milliseconds.
        recorded_ms: u64,
    },
    /// The clip reached its cap and the device is closed; finish to get it.
    Full {
        /// Audio recorded, in milliseconds.
        recorded_ms: u64,
    },
    /// The device failed and is closed; finishing reports the cause.
    Failed(CaptureError),
}

/// Why a recorder command was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RecordError {
    /// A recording is already running.
    #[error("a recording is already running")]
    AlreadyRecording,
    /// Nothing is being recorded.
    #[error("nothing is being recorded")]
    NotRecording,
    /// The microphone failed.
    #[error(transparent)]
    Capture(#[from] CaptureError),
    /// The recording's worker ended abnormally.
    #[error("the recording ended unexpectedly; try again")]
    Worker,
}

#[derive(Debug, Default)]
struct Progress {
    recorded_ms: AtomicU64,
    full: AtomicBool,
}

type Device = Arc<Mutex<Option<OpenCapture>>>;

struct Session {
    device: Device,
    worker: JoinHandle<ClipBuilder>,
    progress: Arc<Progress>,
    failure: Arc<Mutex<Option<CaptureError>>>,
    overruns: Arc<AtomicU64>,
}

/// Records one clip at a time.
pub struct Recorder {
    capture: Arc<dyn Capture>,
    cap: usize,
    session: Mutex<Option<Session>>,
}

impl std::fmt::Debug for Recorder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Recorder").field("status", &self.status()).finish()
    }
}

fn close(device: &Device) {
    let open = device.lock().unwrap_or_else(PoisonError::into_inner).take();
    if let Some(open) = open {
        open.stop();
    }
}

impl Recorder {
    /// A recorder over `capture`, capped at 5 minutes.
    pub fn new(capture: Arc<dyn Capture>) -> Recorder {
        Recorder::with_cap(capture, MAX_CLIP_SAMPLES)
    }

    /// A recorder capped at `cap` samples of 16 kHz (never more than 5 minutes).
    pub fn with_cap(capture: Arc<dyn Capture>, cap: usize) -> Recorder {
        Recorder {
            capture,
            cap: cap.min(MAX_CLIP_SAMPLES),
            session: Mutex::new(None),
        }
    }

    fn session(&self) -> std::sync::MutexGuard<'_, Option<Session>> {
        self.session.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Opens the microphone and starts recording.
    pub fn start(&self) -> Result<(), RecordError> {
        let mut slot = self.session();
        if slot.is_some() {
            return Err(RecordError::AlreadyRecording);
        }
        let (sink, hand_over) = Frames::channel(CHANNEL_CHUNKS);
        let (overruns, failure) = (hand_over.overruns.clone(), hand_over.failure.clone());
        let open = self.capture.open(sink)?;
        let format = open.format();
        let device: Device = Arc::new(Mutex::new(Some(open)));
        let progress = Arc::new(Progress::default());
        let (dev, prog, fail, cap) = (device.clone(), progress.clone(), failure.clone(), self.cap);
        let worker = std::thread::Builder::new().name("pitwall-speech-capture".into()).spawn(move || {
            let mut clip = ClipBuilder::new(format, cap);
            // Ends when the device is closed (its Frames dropped) and every waiting chunk is read.
            while let Ok(chunk) = hand_over.rx.recv() {
                if let Chunk::Samples(v) = chunk {
                    if !prog.full.load(Ordering::SeqCst) {
                        let push = clip.push(&v);
                        prog.recorded_ms.store(clip.duration_ms(), Ordering::SeqCst);
                        if push == Push::Full {
                            prog.full.store(true, Ordering::SeqCst);
                            close(&dev);
                        }
                    }
                    // The buffer goes back to the audio thread, which then never allocates.
                    hand_over.give_back(v);
                }
                if fail.lock().unwrap_or_else(PoisonError::into_inner).is_some() {
                    close(&dev);
                }
            }
            clip
        });
        let worker = match worker {
            Ok(w) => w,
            Err(e) => {
                close(&device);
                return Err(CaptureError::Device(format!("no recording thread: {e}")).into());
            }
        };
        log::info!(
            target: "pitwall_speech",
            "recording started: {} Hz, {} channels",
            format.sample_rate(),
            format.channels()
        );
        *slot = Some(Session {
            device,
            worker,
            progress,
            failure,
            overruns,
        });
        Ok(())
    }

    /// Where the recorder is.
    pub fn status(&self) -> Status {
        let slot = self.session();
        let Some(s) = slot.as_ref() else {
            return Status::Idle;
        };
        if let Some(e) = s.failure.lock().unwrap_or_else(PoisonError::into_inner).clone() {
            return Status::Failed(e);
        }
        let recorded_ms = s.progress.recorded_ms.load(Ordering::SeqCst);
        if s.progress.full.load(Ordering::SeqCst) {
            Status::Full { recorded_ms }
        } else {
            Status::Recording { recorded_ms }
        }
    }

    fn end(&self) -> Result<(ClipBuilder, Option<CaptureError>, u64), RecordError> {
        let session = self.session().take().ok_or(RecordError::NotRecording)?;
        close(&session.device);
        let clip = session.worker.join().map_err(|_| RecordError::Worker)?;
        let failure = session.failure.lock().unwrap_or_else(PoisonError::into_inner).clone();
        Ok((clip, failure, session.overruns.load(Ordering::Relaxed)))
    }

    /// Stops recording and returns the clip, or the device's failure.
    pub fn finish(&self) -> Result<Clip, RecordError> {
        let (clip, failure, overruns) = self.end()?;
        let clip = clip.finish();
        match failure {
            Some(e) => {
                log::warn!(target: "pitwall_speech", "recording failed after {} ms: {e}", clip.duration_ms());
                Err(e.into())
            }
            None => {
                log::info!(
                    target: "pitwall_speech",
                    "recording finished: {} ms, {overruns} overruns",
                    clip.duration_ms()
                );
                Ok(clip)
            }
        }
    }

    /// Stops recording and throws the clip away. Nothing happens when nothing is being recorded.
    pub fn cancel(&self) {
        if let Ok((clip, _, _)) = self.end() {
            log::info!(target: "pitwall_speech", "recording discarded: {} ms", clip.duration_ms());
        }
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.cancel();
    }
}
